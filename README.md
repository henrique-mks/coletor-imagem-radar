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
- **Fase 3 (catálogo Postgres + dedupe persistente)** ✅ — catálogo no nosso Postgres (schema
  `imagens_satelite`, hypertable TimescaleDB, via SeaORM; migration pelo subcomando `migrate`) +
  cache local `redb` pro dedupe e janela de overlap. O dedupe é persistente: o catálogo é a fonte de
  verdade e o redb é hidratado no boot com a janela recente (~48h).
- **Lado consumidor (gRPC `serve`)** ✅ — servidor tonic que **consulta** o catálogo (sem Kafka, sem
  push). Duas RPCs unárias — `UltimoFrame(produto, canal?)` e `ListarFrames(...)` (janela temporal,
  paginada por cursor). Devolve **só metadado** + uma **URL pré-assinada** (GET S3) do `.pmtiles`; os
  bytes trafegam por HTTP range request direto do bucket, nunca pelo gRPC.
- **Empacotamento OCI** ✅ — `Containerfile` multi-stage + `compose.yaml`; CI no GitHub Actions.

Fases seguintes: GLM/multiproduto e hardening (métricas/observabilidade). Plano completo no Obsidian:
`Projetos/Sincronizador GOES-19 NODD/Plano`.

## Uso

```sh
cp config.example.toml config.toml   # ajuste buckets/prefixo + seção [database] e [grpc]
cargo run -- check                   # valida config + lista a origem (dry-run, sem escrever)
cargo run -- migrate                 # aplica as migrations do catálogo (schema imagens_satelite)
cargo run -- run --once --limit 1    # uma passada do pipeline (download→processa→upload→catálogo→delete)
cargo run -- run                     # loop contínuo (poll por produto)
cargo run -- backfill --hours 48     # popula retroativo: varre as últimas N horas numa passada
cargo run -- serve                   # servidor gRPC de consulta ao catálogo (UltimoFrame/ListarFrames)
```

Credenciais do **destino S3** vêm SÓ do ambiente (nunca no TOML):

```sh
export AWS_ACCESS_KEY_ID=...
export AWS_SECRET_ACCESS_KEY=...
# AWS_SESSION_TOKEN opcional
```

Credenciais do **Postgres** (catálogo) vêm da seção `[database]` do TOML. Os subcomandos `run`,
`migrate` e `serve` exigem essa seção; `check` e `backfill` (dry-run) não.

Logging: `RUST_LOG=coletor_imagem_radar=debug` para verbosidade; `SYNC_LOG_FORMAT=json` para JSON.

### Dependências externas

O processamento **shella binários** GDAL (`gdal_calc.py`, `gdalwarp`, `gdaldem`, `gdal_translate`,
`gdaladdo`) + `pmtiles` — precisam estar no `PATH` para o `run` (não para `build`/`test`).
`run`/`migrate`/`serve` precisam de um **Postgres** acessível. O `serve` ainda precisa das credenciais
AWS do destino para **pré-assinar** as URLs GET — e a identidade IAM que assina precisa de
`s3:GetObject` no prefixo (o usuário só-`PutObject` gera URLs que voltam **403** no fetch).

## Layout

| Módulo          | Papel                                                        |
|-----------------|--------------------------------------------------------------|
| `main.rs`       | CLI clap (`check`, `run`, `backfill`, `migrate`, `serve`).   |
| `config.rs`     | Carga/validação do TOML (`[database]`, `[grpc]`).            |
| `storage.rs`    | Constrói os clients `object_store` (origem anônima / destino) + signer p/ presign.|
| `nodd.rs`       | Convenções de chave do NODD (prefixo `AAAA/DDD/HH`) + parser de timestamp.|
| `pipeline.rs`   | Loop poll → download → processa → upload → catálogo → delete-on-success.|
| `process.rs`    | Cadeia GDAL → PMTiles do C13.                                |
| `state.rs`      | Estado híbrido: catálogo Postgres (SeaORM) + cache redb; dedupe + migrations.|
| `query.rs`      | Queries read-only do catálogo p/ o gRPC (`ultimo_frame`, `listar_frames`).|
| `serve.rs`      | Servidor gRPC (tonic): impl do serviço `Catalogo` + presign de URL.|
| `grpc.rs`       | Código gerado do `.proto` (`include_proto!`).               |
| `entity.rs`     | Entidade SeaORM da tabela `frames`.                         |
| `logging.rs`    | Init do `tracing` (texto ou JSON).                          |
