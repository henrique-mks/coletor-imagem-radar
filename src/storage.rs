//! Construção dos clients S3 via `object_store`.
//!
//! - **Origem** (NODD): anônima, `skip_signature(true)`. Sem credenciais.
//! - **Destino** (espelho): credenciais do ambiente (`AWS_*`), com `endpoint`
//!   override opcional para MinIO/S3-compatível.

use std::sync::Arc;

use anyhow::{Context, Result};
use object_store::ObjectStore;
use object_store::aws::AmazonS3Builder;
use object_store::local::LocalFileSystem;

use crate::config::{DestinationConfig, SourceConfig};

/// Client de leitura anônima do bucket público do NODD.
pub fn build_source(cfg: &SourceConfig) -> Result<Arc<dyn ObjectStore>> {
    let store = AmazonS3Builder::new()
        .with_bucket_name(&cfg.bucket)
        .with_region(&cfg.region)
        // Bucket público: requisições não assinadas.
        .with_skip_signature(true)
        .build()
        .with_context(|| format!("construindo client de origem para '{}'", cfg.bucket))?;
    Ok(Arc::new(store))
}

/// Client de escrita do bucket de destino.
///
/// Credenciais vêm do ambiente (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`,
/// opcional `AWS_SESSION_TOKEN`) via [`AmazonS3Builder::from_env`]. A config
/// (bucket/região/endpoint) sobrescreve o que vier do ambiente.
pub fn build_destination(cfg: &DestinationConfig) -> Result<Arc<dyn ObjectStore>> {
    // Destino local (dev/teste): grava no filesystem sob `local_path`.
    if let Some(path) = &cfg.local_path {
        std::fs::create_dir_all(path)
            .with_context(|| format!("criando diretório de destino local '{path}'"))?;
        let store = LocalFileSystem::new_with_prefix(path)
            .with_context(|| format!("destino local em '{path}'"))?;
        return Ok(Arc::new(store));
    }

    let mut builder = AmazonS3Builder::from_env()
        .with_bucket_name(&cfg.bucket)
        .with_region(&cfg.region);

    if let Some(endpoint) = &cfg.endpoint {
        if !endpoint.is_empty() {
            builder = builder.with_endpoint(endpoint).with_virtual_hosted_style_request(false);
        }
    }
    if cfg.allow_http {
        builder = builder.with_allow_http(true);
    }

    let store = builder
        .build()
        .with_context(|| format!("construindo client de destino para '{}'", cfg.bucket))?;
    Ok(Arc::new(store))
}
