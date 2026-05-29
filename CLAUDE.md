# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## O que é

`sync-goes19` é um binário Rust (tokio) que espelha produtos GOES-19 do **NOAA
NODD** (bucket público `noaa-goes19`, `us-east-1`, leitura anônima) para um S3
nosso (AWS, MinIO ou filesystem local) e, no caminho, **processa** cada frame
ABI C13 (NetCDF) em **PMTiles** prontos para mapa. Comentários e docs em pt-BR.

## Comandos

```sh
cargo build                         # compila
cargo run -- check --limit 5        # valida config + lista a origem (dry-run, sem escrever)
cargo run -- run --once --limit 1   # uma passada do pipeline (download→processa→upload→delete)
cargo run -- run                    # loop contínuo (poll por produto)
cargo test                          # testes (hoje só em src/nodd.rs)
cargo test source_hour_prefix       # roda um teste específico por nome
```

- Config: `-c/--config` (default `config.toml`). Copie `config.example.toml`.
- Credenciais do destino vêm SÓ do ambiente: `AWS_ACCESS_KEY_ID`,
  `AWS_SECRET_ACCESS_KEY` (`AWS_SESSION_TOKEN` opcional). Nunca no TOML.
- Logs: `RUST_LOG=sync_goes19=debug` para verbosidade; `SYNC_LOG_FORMAT=json`
  para saída JSON.

## Dependências externas (não-Rust)

O processamento **shella binários** — precisam estar no `PATH`, senão `run`
falha em runtime (não em compile):
`gdal_calc.py`, `gdalwarp`, `gdaldem`, `gdal_translate`, `gdaladdo` (pacote GDAL)
e `pmtiles` (conversor MBTiles→PMTiles). `cargo build`/`cargo test` NÃO precisam
deles; só `cargo run -- run`.

## Arquitetura

Fluxo do pipeline (`src/pipeline.rs` → `src/process.rs`), por produto e por poll:

1. **Poll**: lista o prefixo da hora UTC corrente na origem
   (`nodd::source_hour_prefix`, layout NODD `<Produto>/<AAAA>/<DDD>/<HH>/`, onde
   DDD é dia juliano). Filtra por canal via substring `"<canal>_G19"` (ex.
   `C13_G19`); produto sem `channel` não filtra.
2. **Download**: GET anônimo em stream → disco efêmero (`pipeline.work_dir`,
   default `data/`). Pula se o `.nc` já existe em disco.
3. **Processa** (`process::process` despacha por `product.name`): só
   `abi-l2-cmipf-c13` tem pipeline. Cadeia GDAL: calibração CMI→°C
   (`gdal_calc.py`) → reproj/recorte EPSG:3857 no BBOX (`gdalwarp`) → colormap
   NOAA (`gdaldem color-relief` + rampa `assets/c13_noaa.txt`) → MBTiles
   (`gdal_translate` + `gdaladdo`) → PMTiles (`pmtiles convert`).
4. **Upload**: PUT do `.pmtiles` no destino, sob a chave de
   `nodd::dest_pmtiles_key` (reaproveita `AAAA/DDD/HH` da origem, troca extensão).
5. **Delete-on-success**: só após o upload OK apaga o `.nc` e o `.pmtiles` local.
   Erro NÃO marca como visto → retentado no próximo poll.

Dedupe é **em memória** (`HashSet` em `pipeline::run`) — some ao reiniciar
(persistência é trabalho futuro).

Módulos:

| Módulo         | Papel |
|----------------|-------|
| `main.rs`      | CLI clap (`check`, `run`); subcomando `check` faz o smoke-test de list. |
| `config.rs`    | Structs serde do TOML + `Config::load`/`validate`. `deny_unknown_fields`. |
| `storage.rs`   | Constrói clients `object_store`: origem anônima (`skip_signature`), destino S3/MinIO (`from_env` + `endpoint`/`allow_http`) ou `LocalFileSystem` quando `destination.local_path` está setado. |
| `nodd.rs`      | Convenções de chave NODD (prefixo da hora, chave de destino). Tem os testes. |
| `pipeline.rs`  | Loop poll→processa por produto; dedupe em memória. |
| `process.rs`   | Cadeia GDAL→PMTiles do C13; constantes de calibração/BBOX/resolução. |
| `logging.rs`   | Init do `tracing` (texto ou JSON). |

## Constantes do C13 (em `src/process.rs`)

Calibração `SCALE`/`OFFSET` (Kelvin→°C), `BBOX = [-100, -56, -20, 13]`
(EPSG:4326; América do Sul + Atlântico, estendido a oeste até ~Cidade do México
a pedido dos meteorologistas) e `TARGET_RES_M = "2000"` (~2 km em 3857). Mudar
a cobertura/resolução = editar essas constantes.

## Notas

- Rust edition 2024.
- `config.toml`, `target/`, `data/`, `out-s3/`, `temp/` são gitignored.
- O pipeline reaproveita a cadeia do PoC `goes-nodd-poc`, trocando a cauda COG
  por PMTiles.
