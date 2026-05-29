//! Configuração do sincronizador.
//!
//! Carregada de um arquivo TOML (ver `config.example.toml`). As credenciais do
//! destino NÃO ficam aqui — vêm do ambiente (`AWS_ACCESS_KEY_ID` /
//! `AWS_SECRET_ACCESS_KEY`), lidas por `object_store` em [`crate::storage`].

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

/// Configuração raiz do sincronizador.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub source: SourceConfig,
    pub destination: DestinationConfig,
    /// Produtos a processar. Vazio é válido no esqueleto (Fase 1).
    #[serde(default)]
    pub products: Vec<ProductConfig>,
    /// Parâmetros do pipeline de processamento (Fase 2+).
    #[serde(default)]
    pub pipeline: PipelineConfig,
}

/// Parâmetros do pipeline de ingest+processamento.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineConfig {
    /// Diretório de trabalho para o bruto efêmero e artefatos intermediários.
    #[serde(default = "default_work_dir")]
    pub work_dir: String,
    /// Arquivo de rampa de cor (gdaldem color-relief) para o C13. Em °C.
    #[serde(default = "default_c13_ramp")]
    pub c13_color_ramp: String,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            work_dir: default_work_dir(),
            c13_color_ramp: default_c13_ramp(),
        }
    }
}

/// Bucket de origem no NOAA NODD — leitura anônima, sem assinatura.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceConfig {
    /// Ex.: `noaa-goes19`.
    pub bucket: String,
    /// Região do bucket público. NODD vive em `us-east-1`.
    #[serde(default = "default_region")]
    pub region: String,
}

/// Bucket de destino (espelho). AWS S3 real ou MinIO/S3-compatível via `endpoint`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DestinationConfig {
    pub bucket: String,
    #[serde(default = "default_region")]
    pub region: String,
    /// Endpoint customizado para MinIO ou S3-compatível.
    /// `None`/ausente = AWS S3 real. Ex. MinIO: `http://localhost:9000`.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Permite HTTP (sem TLS) — necessário para MinIO local. Ignorado em AWS.
    #[serde(default)]
    pub allow_http: bool,
    /// Se definido, grava no **filesystem local** sob este caminho (dev/teste),
    /// ignorando S3/`endpoint`. O `prefix` ainda é aplicado às chaves.
    #[serde(default)]
    pub local_path: Option<String>,
    /// Prefixo-raiz das chaves no destino. Ex.: `goes19`.
    /// O layout normalizado do plano é montado a partir daqui.
    #[serde(default)]
    pub prefix: String,
}

/// Um produto GOES-19 a espelhar (ex.: ABI C13 full disk, GLM LCFA).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductConfig {
    /// Identificador interno/legível. Ex.: `abi-l2-cmipf-c13`.
    pub name: String,
    /// Prefixo do produto no bucket de origem. Ex.: `ABI-L2-CMIPF`.
    pub source_prefix: String,
    /// Canal ABI a filtrar (ex.: `C13`). `None` = sem filtro de canal (ex.: GLM).
    #[serde(default)]
    pub channel: Option<String>,
    /// Intervalo de polling em segundos (a cadência real é por produto).
    #[serde(default = "default_poll_secs")]
    pub poll_interval_secs: u64,
}

fn default_region() -> String {
    "us-east-1".to_string()
}

fn default_poll_secs() -> u64 {
    120
}

fn default_work_dir() -> String {
    "data".to_string()
}

fn default_c13_ramp() -> String {
    "assets/c13_noaa.txt".to_string()
}

impl Config {
    /// Lê e valida a configuração de um arquivo TOML.
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("lendo config em {}", path.display()))?;
        let config: Config =
            toml::from_str(&raw).with_context(|| format!("parseando TOML de {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        anyhow::ensure!(!self.source.bucket.is_empty(), "source.bucket vazio");
        anyhow::ensure!(
            !self.destination.bucket.is_empty(),
            "destination.bucket vazio"
        );
        for p in &self.products {
            anyhow::ensure!(!p.name.is_empty(), "product.name vazio");
            anyhow::ensure!(
                !p.source_prefix.is_empty(),
                "product.source_prefix vazio para '{}'",
                p.name
            );
        }
        Ok(())
    }
}
