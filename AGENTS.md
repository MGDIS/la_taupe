# AGENTS.md

Guide pour agents et développeurs travaillant sur **La Taupe**.

## Vue d'ensemble

La Taupe est un service HTTP (Rust / actix-web) qui extrait des données administratives de documents (images, PDF). Deux types d'analyse :

- **RIB** : extraction IBAN / BIC / titulaire via OCR.
- **2D-Doc** : lecture et vérification des codes-barres 2D-Doc de l'ANTS (datamatrix), avec vérification de signature contre les certificats de confiance.

L'OCR repose sur deux moteurs complémentaires : **PP-OCR v6 tiny** (crate `oar-ocr`, ONNX Runtime CPU lié statiquement, modèles `.onnx` téléchargés au build et embarqués via `include_bytes!`) pour la page, et **Tesseract** (via `leptess`, in-process) en repli sur les recadrages d'IBAN et pour l'angle de la page (hOCR). L'extraction texte des PDF utilise **poppler** (`pdftotext`) ; le rendu PDF→image passe aussi par poppler.

## Structure du code

- `src/main.rs` — point d'entrée CLI : sans argument démarre le serveur, avec des chemins (fichiers ou répertoires) lance une analyse batch (une ligne JSON par fichier). Options `--type rib|2ddoc`, `--jobs N`, `--trace`, `--split-holder`, `--version`, `--help`, `--trusted-repositories-urls`.
- `src/http/` — couche HTTP : `server.rs` (bootstrap, `AppState`, config multipart), `analyze.rs` (endpoints `/analyze` et `/analyze/upload`), `ping.rs`, `version.rs`.
- `src/config.rs` — `AppConfig::from_env()` : centralise la lecture des variables d'environnement (voir plus bas).
- `src/analysis.rs` — orchestration de l'analyse (`Analysis`, `Hint`, `Type`).
- `src/pool.rs` — pool global synchrone d'instances Tesseract (`LepTessPool`), créées à la demande jusqu'à `LA_TAUPE_MAX_CONCURRENT_OCR`, empruntées le temps d'un appel (RAII). PP-OCR n'a pas de pool : une session partagée (`src/ppocr.rs`).
- `src/twoddoc/` — parsing 2D-Doc, structure des données, certificats et signature.
- `src/rib.rs`, `src/ocr.rs` (cascade), `src/ppocr.rs`, `src/tesseract.rs`, `src/lines.rs`, `src/address_blocks.rs`, `src/gutters.rs`, `src/text/` — extraction RIB, titulaire et primitives OCR.
- `src/provenance.rs`, `src/timing.rs` — trace de ce qu'a fait la cascade (`--trace`) et mesure des temps.
- `src/bench/`, `src/synth/`, `src/bin/{bench,synth}.rs` — banc de mesure et corpus synthétique, derrière la feature `bench` (hors binaire de prod).
- `src/file_utils.rs`, `src/image_utils.rs`, `src/datamatrix.rs` — utilitaires fichiers / images / datamatrix.
- `build.rs` — injecte `GIT_HASH` et télécharge les modèles PP-OCR en profil release (`download-models.sh`, vérification SHA-256). **Refuse un build release avec des modifications non commitées** (hors Docker, où `.git` est absent). En profil debug, crée des modèles vides pour que `include_bytes!` compile.
- `rust-toolchain.toml` — toolchain épinglée (1.97.1).

## Endpoints HTTP

- `POST /analyze` — corps JSON `{"url": "...", "hint": {"type": "rib"|"2ddoc"}}`. Télécharge le fichier distant (streaming, borné par `max_file_size`) puis l'analyse.
- `POST /analyze/upload` — multipart : champ `file` (binaire) + champ optionnel `hint` (`rib` ou `2ddoc`). Limite de taille via `MultipartFormConfig`.
- `GET /ping` — healthcheck. Renvoie `404` si un fichier `maintenance` existe dans le répertoire courant, `200` sinon.
- `GET /version` — versions de La Taupe (git hash) et de Tesseract.

Le `hint` est optionnel ; sans lui, l'analyse tente à la fois 2D-Doc et RIB.

## Variables d'environnement

Toutes lues dans `src/config.rs` (sauf `LA_TAUPE_ADDRESS` dans `server.rs`). Valeur invalide ou absente → défaut.

| Variable | Défaut | Description |
|---|---|---|
| `LA_TAUPE_WORKERS` | `3` | Nombre de workers actix-web. |
| `LA_TAUPE_MAX_CONCURRENT_OCR` | `3` | OCR concurrents max (taille du sémaphore et des pools). |
| `LA_TAUPE_OCR_TIMEOUT_SECS` | `120` | Timeout d'un traitement OCR (secondes). |
| `LA_TAUPE_MAX_FILE_SIZE` | `10` | Taille max de fichier acceptée, **en mégaoctets** (convertie en octets en interne). S'applique à l'upload multipart et au téléchargement distant. |
| `LA_TAUPE_ADDRESS` | `127.0.0.1:8080` | Adresse d'écoute du serveur. |
| `LA_TAUPE_PPOCR_MODEL_DIR` | — | (upstream, optionnel) répertoire contenant `det.onnx`, `rec.onnx`, `dict.txt` pour essayer d'autres modèles PP-OCR sans rebuild. Lu dans `src/ppocr.rs`. |
| `RUST_LOG` | `info` | Niveau de log (env_logger). |

## Build & exécution

Dépendances système requises : `pkg-config`, `libssl-dev`, `libtesseract-dev`, `libleptonica-dev`, `libclang-dev`, et au runtime `poppler-utils`, `tesseract-ocr`, `tesseract-ocr-fra`.

```bash
cargo build --release        # build (télécharge les modèles PP-OCR si absents ; refuse si arbre non commité)
./download-models.sh         # télécharger les modèles manuellement si besoin
cargo run                    # démarre le serveur
./launch_demo.sh             # démo : serveur node factice + la_taupe + curl sur un RIB
```

Le `Dockerfile` (multi-stage) produit l'image de production ; le service tourne en utilisateur non-root sur le port `8080`.

## Tests

**Les tests se lancent via `Dockerfile.test`** — l'image installe toutes les dépendances natives (Tesseract, Leptonica, poppler), le serveur node de fixtures, et télécharge les modèles. C'est la façon de référence pour reproduire l'environnement CI.

```bash
docker build -f Dockerfile.test -t la_taupe_test .
docker run --rm la_taupe_test
```

`Dockerfile.test` a pour `CMD` `cargo test --release`, ce qui exécute :
- les **tests unitaires** (lib), dont `src/config.rs` (parsing des variables d'environnement) ;
- les **tests d'intégration** `tests/http_tests.rs`, qui démarrent le serveur et un serveur node de fixtures (`tests/fixtures/static_server/`) et couvrent les cas nominaux, fichier trop gros, timeout OCR, erreurs upstream, etc.

En local, les tests unitaires purs peuvent tourner directement (`cargo test --lib config`) si les libs natives sont installées, mais les tests d'intégration supposent l'environnement complet — préférer Docker.

## Conventions

- Toujours lancer `cargo fmt` et `cargo clippy --all-targets` avant de committer (le CI vérifie `cargo fmt --all -- --check` et `cargo clippy`).
- Ne jamais committer directement sur `main` : créer une branche `feat/…` (ou équivalent).
- Nouveaux réglages runtime : les ajouter à `AppConfig` (`src/config.rs`) avec un préfixe `LA_TAUPE_`, un défaut, et des tests dans les 3 cas (défaut / custom / valeur invalide).
