# Coletor de Imagem de Radar

> Crate/binário: `coletor-imagem-radar`.

Pipeline em Rust que **ingere** produtos GOES-19 do **NOAA NODD** (`noaa-goes19`, S3 público,
`us-east-1`, leitura anônima), **processa** cada frame ABI C13 (NetCDF) em **PMTiles** prontos para
mapa e os entrega no **nosso S3** (AWS). O bruto (`.nc`) é **efêmero**: baixado para disco, processado
e **descartado após o upload** — não há espelho persistente.

Projeto separado da `qualle-control-api`; primeiro bloco do produto GOES-19.
Stack: **Rust + tokio + `object_store` + GDAL**.

> Plano completo no Obsidian: `Projetos/Sincronizador GOES-19 NODD/Plano`.

## Status

- **Fase 1 (esqueleto)** ✅ — config, clients S3 (origem anônima / destino), logging estruturado e
  um `check` de conectividade.
- **Fase 2 (loop end-to-end C13)** ✅ — `run` faz poll → download → processa (GDAL → PMTiles) →
  upload → delete-on-success.
- **Fase 3 (catálogo Postgres + dedupe persistente)** 🚧 — catálogo no nosso Postgres (schema
  `imagens_satelite`, via SeaORM, migration pelo subcomando `migrate`) + cache local `redb` pro
  dedupe e janela de overlap. Hoje o dedupe é só em memória.

Fases seguintes: GLM/multiproduto, backfill, hardening (métricas, gRPC pro lado consumidor,
containerização). Plano completo no Obsidian: `Projetos/Sincronizador GOES-19 NODD/Plano`.

## Uso

```sh
cp config.example.toml config.toml   # ajuste buckets/prefixo
cargo run -- check                   # valida config + lista a origem (dry-run)
```

Credenciais do destino vêm do ambiente:

```sh
export AWS_ACCESS_KEY_ID=...
export AWS_SECRET_ACCESS_KEY=...
```

Logging: `RUST_LOG=coletor_imagem_radar=debug` para verbosidade; `SYNC_LOG_FORMAT=json` para JSON.

## Layout

| Módulo          | Papel                                                        |
|-----------------|--------------------------------------------------------------|
| `config.rs`     | Carga/validação do TOML.                                     |
| `storage.rs`    | Constrói os clients `object_store` (origem anônima / destino).|
| `nodd.rs`       | Convenções de chave do NODD (prefixo `AAAA/DDD/HH`).         |
| `pipeline.rs`   | Loop poll → download → processa → upload → delete-on-success.|
| `process.rs`    | Cadeia GDAL → PMTiles do C13.                                |
| `logging.rs`    | Init do `tracing`.                                           |
| `main.rs`       | CLI (`check`, `run`).                                        |
