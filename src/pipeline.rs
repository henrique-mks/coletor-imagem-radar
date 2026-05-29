//! Loop de ingest+processamento (reto, sem fila — para testes iniciais).
//!
//! Por produto: poll do prefixo da hora UTC corrente → para cada `.nc` ainda
//! não processado: download p/ disco efêmero → processa → upload do PMTiles →
//! **delete-on-success** do bruto. Dedupe em memória (persistência fica p/ a
//! Fase 3).

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use bytes::Bytes;
use futures::StreamExt;
use object_store::{ObjectStore, ObjectStoreExt, PutPayload, path::Path as ObjPath};
use time::OffsetDateTime;
use tokio::io::AsyncWriteExt;
use tracing::{error, info, warn};

use crate::config::{Config, ProductConfig};
use crate::process::{self, Job};
use crate::{nodd, storage};

/// Roda o pipeline. `once = true` faz uma única passada e sai (para testes).
/// `limit = 0` processa todos os objetos novos por poll; >0 limita.
pub async fn run(config: &Config, once: bool, limit: usize) -> Result<()> {
    let source = storage::build_source(&config.source)?;
    let dest = storage::build_destination(&config.destination)?;

    let work_dir = Path::new(&config.pipeline.work_dir).to_path_buf();
    tokio::fs::create_dir_all(&work_dir).await?;
    let ramp = Path::new(&config.pipeline.c13_color_ramp).to_path_buf();

    if config.products.is_empty() {
        warn!("nenhum produto configurado — nada a processar");
        return Ok(());
    }

    // Dedupe em memória (Fase 3 persiste em redb/SQLite).
    let mut seen: HashSet<String> = HashSet::new();

    loop {
        for product in &config.products {
            if let Err(e) =
                poll_product(&source, &dest, config, product, &work_dir, &ramp, limit, &mut seen)
                    .await
            {
                error!(product = %product.name, error = %format!("{e:#}"), "falha no poll");
            }
        }

        if once {
            break;
        }

        let secs = config
            .products
            .iter()
            .map(|p| p.poll_interval_secs)
            .min()
            .unwrap_or(120);
        info!(secs, "aguardando próximo poll");
        tokio::time::sleep(Duration::from_secs(secs)).await;
    }

    Ok(())
}

/// Lista a hora corrente do produto e processa o que for novo.
async fn poll_product(
    source: &Arc<dyn ObjectStore>,
    dest: &Arc<dyn ObjectStore>,
    config: &Config,
    product: &ProductConfig,
    work_dir: &Path,
    ramp: &Path,
    limit: usize,
    seen: &mut HashSet<String>,
) -> Result<()> {
    let prefix = nodd::source_hour_prefix(product, OffsetDateTime::now_utc());
    info!(product = %product.name, %prefix, "poll");

    // Filtro de canal: nome traz "<canal>_G19" (ex.: M6C13_G19). Sem canal = sem filtro.
    let needle = product.channel.as_ref().map(|c| format!("{c}_G19"));

    let mut stream = source.list(Some(&ObjPath::from(prefix.clone())));
    let mut keys: Vec<String> = Vec::new();
    while let Some(item) = stream.next().await {
        let meta = item.context("listando origem")?;
        let key = meta.location.to_string();
        let matches = needle.as_ref().map(|n| key.contains(n)).unwrap_or(true);
        if matches && !seen.contains(&key) {
            keys.push(key);
        }
    }

    if keys.is_empty() {
        info!(product = %product.name, "nada novo nesta hora");
        return Ok(());
    }
    // Ordena por chave (≈ cronológico, pois o nome começa com s<timestamp>).
    keys.sort();
    if limit > 0 && keys.len() > limit {
        keys.truncate(limit);
    }
    info!(product = %product.name, novos = keys.len(), "objetos a processar");

    for key in keys {
        match process_one(source, dest, config, product, work_dir, ramp, &key).await {
            Ok(()) => {
                seen.insert(key);
            }
            Err(e) => {
                // Não marca como visto → re-tentado no próximo poll (bruto local
                // permanece, sem re-download se ainda existir em disco).
                error!(key = %key, error = %format!("{e:#}"), "falha ao processar objeto");
            }
        }
    }
    Ok(())
}

/// Sequência completa de um objeto: download → processa → upload → delete.
async fn process_one(
    source: &Arc<dyn ObjectStore>,
    dest: &Arc<dyn ObjectStore>,
    config: &Config,
    product: &ProductConfig,
    work_dir: &Path,
    ramp: &Path,
    key: &str,
) -> Result<()> {
    let filename = key.rsplit('/').next().unwrap_or("frame.nc");
    let local_nc = work_dir.join(filename);

    // 1. Download → disco efêmero (cache: pula se já existe).
    if tokio::fs::try_exists(&local_nc).await.unwrap_or(false) {
        info!(file = %filename, "bruto já em disco, pulando download");
    } else {
        download(source, key, &local_nc).await.context("download")?;
    }

    // 2. Processa → PMTiles.
    let job = Job {
        product_name: product.name.clone(),
        source_key: key.to_string(),
        local_nc: local_nc.clone(),
    };
    let pmtiles = process::process(&job, work_dir, ramp)
        .await
        .context("processamento")?;

    // 3. Upload do PMTiles → nosso S3.
    let dest_key = nodd::dest_pmtiles_key(product, &config.destination.prefix, key);
    let bytes = tokio::fs::read(&pmtiles).await.context("lendo PMTiles")?;
    let size = bytes.len();
    dest.put(&ObjPath::from(dest_key.clone()), PutPayload::from(Bytes::from(bytes)))
        .await
        .with_context(|| format!("upload para {dest_key}"))?;
    info!(dest_key = %dest_key, bytes = size, "PMTiles no destino");

    // 4. Delete-on-success: só agora apaga o bruto e o PMTiles local.
    tokio::fs::remove_file(&local_nc).await.ok();
    tokio::fs::remove_file(&pmtiles).await.ok();
    info!(file = %filename, "bruto descartado (pós-upload)");

    Ok(())
}

/// Stream do GET anônimo direto para arquivo, sem buffer integral em memória.
async fn download(source: &Arc<dyn ObjectStore>, key: &str, dest: &Path) -> Result<()> {
    info!(key = %key, dest = %dest.display(), "baixando");
    let result = source.get(&ObjPath::from(key)).await.context("GET origem")?;
    let mut stream = result.into_stream();
    let mut file = tokio::fs::File::create(dest).await?;
    let mut total: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.context("lendo chunk")?;
        total += bytes.len() as u64;
        file.write_all(&bytes).await?;
    }
    file.flush().await?;
    info!(bytes = total, "download concluído");
    Ok(())
}
