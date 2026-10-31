//! Models Carrel can download for local providers (`docs/design/models.md`).
//!
//! [`CATALOG`] lists the models Carrel knows work, each pinned to one file at a fixed commit
//! with its size and SHA-256. Downloading goes through the network helper, `carrel-fetch`,
//! the only Carrel program that goes online: [`download`] hands it a [`FetchRequest`], reads
//! its [`FetchEvent`]s, then checks the file itself before giving it the name the llama.cpp
//! provider looks for.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// A model in the catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Model {
    /// Its short name, used in commands and as its file's name.
    pub name: &'static str,
    /// What it is, in words.
    pub title: &'static str,
    /// The Hugging Face repository holding it.
    pub repository: &'static str,
    /// The repository's commit the file is pinned to.
    pub revision: &'static str,
    /// The file's name in the repository.
    pub file: &'static str,
    /// The file's size in bytes.
    pub size: u64,
    /// The file's SHA-256, in lowercase hexadecimal.
    pub sha256: &'static str,
    /// Its license's SPDX identifier.
    pub license: &'static str,
    /// The memory a machine should have to run it, in gigabytes.
    pub memory_gb: u8,
    /// Whether the benchmark recommends it for machines with that much memory
    /// (`docs/bench/`).
    pub recommended: bool,
    /// The same model converted for MLX, for `carrel-provider-mlx` (`docs/design/mlx.md`).
    pub mlx: Option<MlxBuild>,
    /// For an embedding model, how to use it; `None` for a text model.
    pub embedding: Option<Embedding>,
    /// The provider a model comes with, such as Apple's sentence embedding with
    /// `carrel-provider-apple`: there's nothing to download. `None` for a model to download.
    pub comes_with: Option<&'static str>,
}

/// How to use an embedding model (`docs/design/search.md`). Every provider that runs the
/// model uses it the same way, so vectors from different devices can be compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Embedding {
    /// How many numbers each vector has.
    pub dimensions: usize,
    /// The longest text it reads, in tokens; longer texts are cut.
    pub context_tokens: u32,
    /// How the vectors of a text's tokens become the text's one vector.
    pub pooling: Pooling,
    /// What goes before a search query, as the model was trained.
    pub query_prefix: &'static str,
    /// What goes before a text to be found.
    pub document_prefix: &'static str,
}

/// How an embedding model pools its tokens' vectors into one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Pooling {
    /// The mean of every token's vector.
    Mean,
    /// The last token's vector.
    Last,
    /// The model's own: it gives the text's vector itself, as Apple's sentence embedding does.
    Own,
}

impl Embedding {
    /// `text` with the prefix for `purpose`.
    #[must_use]
    pub fn prompt(&self, text: &str, purpose: crate::providers::Purpose) -> String {
        let prefix = match purpose {
            crate::providers::Purpose::Query => self.query_prefix,
            crate::providers::Purpose::Document => self.document_prefix,
        };
        format!("{prefix}{text}")
    }
}

/// A model converted for MLX: a folder of files rather than one, each pinned like a GGUF file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MlxBuild {
    /// The Hugging Face repository holding it.
    pub repository: &'static str,
    /// The repository's commit the files are pinned to.
    pub revision: &'static str,
    /// The files the provider needs: the configuration, tokenizer and weights.
    pub files: &'static [ModelFile],
}

/// One file of an [`MlxBuild`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ModelFile {
    /// Its path in the repository, which is also its path in the model's folder.
    pub path: &'static str,
    /// Its size in bytes.
    pub size: u64,
    /// Its SHA-256, in lowercase hexadecimal.
    pub sha256: &'static str,
}

impl MlxBuild {
    /// The files' total size in bytes.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    /// Where `file` is downloaded from.
    #[must_use]
    pub fn url(&self, file: &ModelFile) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repository, self.revision, file.path
        )
    }
}

impl Model {
    /// Where the file is downloaded from.
    #[must_use]
    pub fn url(&self) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repository, self.revision, self.file
        )
    }

    /// The name it's saved under in the models folder.
    #[must_use]
    pub fn file_name(&self) -> String {
        format!("{}.gguf", self.name)
    }

    /// Whether its file is pinned to a commit, size and SHA-256, so it can be downloaded.
    #[must_use]
    pub fn is_pinned(&self) -> bool {
        !self.revision.is_empty() && !self.sha256.is_empty() && self.size > 0
    }
}

/// The models Carrel can download. Each is open weights under a permissive license, not gated
/// behind an account, and loadable by the llama.cpp Carrel ships. Their MLX builds are the
/// model makers' own where there is one (IBM's), else `mlx-community`'s 4-bit conversion,
/// with the files `carrel-provider-mlx` reads. The embedding models' are 8-bit, nearest the
/// GGUFs' `Q8_0`, so their vectors stay close to the llama.cpp provider's.
pub const CATALOG: &[Model] = &[
    Model {
        name: "granite-4.2-3b",
        title: "IBM Granite 4.2 3B, Q4_K_M",
        repository: "ibm-granite/granite-4.2-3b-GGUF",
        revision: "c40945d71cd90f249a56985e8155551a9188dc30",
        file: "granite-4.2-3b-Q4_K_M.gguf",
        size: 2_244_011_552,
        sha256: "e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5",
        license: "Apache-2.0",
        memory_gb: 8,
        recommended: false,
        mlx: Some(MlxBuild {
            repository: "ibm-granite/granite-4.2-3b-q4-mlx",
            revision: "0c6f39b1827afd5eb2c1c3b13751929857434953",
            files: &[
                ModelFile {
                    path: "chat_template.jinja",
                    size: 9_013,
                    sha256: "f0ba43f79b3cabca5e5a7584c77aec92f49feae45cdf7779c87d9fc54cd90258",
                },
                ModelFile {
                    path: "config.json",
                    size: 1_185,
                    sha256: "bfcb1a01587dae223af89f07b960f0b456ef9fe9576167a126aa807941b213d7",
                },
                ModelFile {
                    path: "generation_config.json",
                    size: 207,
                    sha256: "1b9baf0a7959d31d7fb58ff6b506c27d09a73d395cadccc7e6dd1d4b7e43fd9d",
                },
                ModelFile {
                    path: "model.safetensors",
                    size: 2_059_005_430,
                    sha256: "a71cf9297827b3c3d6ad426081b67c423044f6576632dabe40e7590552fd267d",
                },
                ModelFile {
                    path: "tokenizer.json",
                    size: 7_153_414,
                    sha256: "883975314d58743726347caaf2c32ac89afd5ad57222cffbc4f052f2ab27b2d7",
                },
                ModelFile {
                    path: "tokenizer_config.json",
                    size: 436,
                    sha256: "0b1cc678b9fbde8ce90c81e0f6483c4e1696dfa377330e42a88106416b96e659",
                },
            ],
        }),
        embedding: None,
        comes_with: None,
    },
    Model {
        name: "qwen3.5-4b",
        title: "Qwen3.5 4B, Q4_K_M",
        repository: "unsloth/Qwen3.5-4B-GGUF",
        revision: "e87f176479d0855a907a41277aca2f8ee7a09523",
        file: "Qwen3.5-4B-Q4_K_M.gguf",
        size: 2_740_937_888,
        sha256: "00fe7986ff5f6b463e62455821146049db6f9313603938a70800d1fb69ef11a4",
        license: "Apache-2.0",
        memory_gb: 8,
        recommended: true,
        mlx: Some(MlxBuild {
            repository: "mlx-community/Qwen3.5-4B-MLX-4bit",
            revision: "32f3e8ecf65426fc3306969496342d504bfa13f3",
            files: &[
                ModelFile {
                    path: "chat_template.jinja",
                    size: 7_756,
                    sha256: "a4aee8afcf2e0711942cf848899be66016f8d14a889ff9ede07bca099c28f715",
                },
                ModelFile {
                    path: "config.json",
                    size: 3_366,
                    sha256: "f3efc81b2ea8d96a45301037d3ccccbcccdef44a961845c87f286aaddbc6eaaa",
                },
                ModelFile {
                    path: "model.safetensors",
                    size: 3_034_300_695,
                    sha256: "5fb9acd0246866381cf8c5c354c6db1019f6498eec4ccb4f5edcc71ffeacb2db",
                },
                ModelFile {
                    path: "model.safetensors.index.json",
                    size: 101_944,
                    sha256: "52e534c41f7b97708329c85f762e5882bf48bd5955a422c6ae74eba321e6048a",
                },
                ModelFile {
                    path: "tokenizer.json",
                    size: 19_989_343,
                    sha256: "87a7830d63fcf43bf241c3c5242e96e62dd3fdc29224ca26fed8ea333db72de4",
                },
                ModelFile {
                    path: "tokenizer_config.json",
                    size: 1_139,
                    sha256: "e98f1901ac6f0adff67b1d540bfa0c36ac1a0cf59eb72ed78146ef89aafa1182",
                },
            ],
        }),
        embedding: None,
        comes_with: None,
    },
    Model {
        name: "gemma-4-e4b",
        title: "Gemma 4 E4B, Google's QAT Q4_0",
        repository: "google/gemma-4-E4B-it-qat-q4_0-gguf",
        revision: "4b4a2c1d584be7264f87aac328a1bc739ce81b6c",
        file: "gemma-4-E4B_q4_0-it.gguf",
        size: 5_154_941_280,
        sha256: "676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee",
        license: "Apache-2.0",
        memory_gb: 16,
        recommended: true,
        mlx: Some(MlxBuild {
            repository: "mlx-community/gemma-4-e4b-it-4bit",
            revision: "475b9088d29754a3379866cf5aeb6b41acd313c2",
            files: &[
                ModelFile {
                    path: "chat_template.jinja",
                    size: 17_336,
                    sha256: "2f1b4d75d067bae3fe44e676721c7f077d243bc007156cb9c2f8b5836613d082",
                },
                ModelFile {
                    path: "config.json",
                    size: 6_628,
                    sha256: "780ccb3a514a5f1ced161d383f948fc22eca9b84b752ca19494f625bd9bad7a6",
                },
                ModelFile {
                    path: "generation_config.json",
                    size: 208,
                    sha256: "d4226bbe3117d2d253ba4609720ba82c6c4ce4627a9a6ae05387c78983ac03de",
                },
                ModelFile {
                    path: "model.safetensors",
                    size: 5_146_800_534,
                    sha256: "932b8271fc3fe65adcc78b96c10c6268bbfb13e8f67d1358727c0d6ee97e1eff",
                },
                ModelFile {
                    path: "model.safetensors.index.json",
                    size: 240_961,
                    sha256: "f8accac59ee7efe87e0c298c854610b262c3cadd477407503147c71209ff0093",
                },
                ModelFile {
                    path: "tokenizer.json",
                    size: 32_169_626,
                    sha256: "cc8d3a0ce36466ccc1278bf987df5f71db1719b9ca6b4118264f45cb627bfe0f",
                },
                ModelFile {
                    path: "tokenizer_config.json",
                    size: 2_740,
                    sha256: "080d9e1aff284e2f6043889cd05367966f7c7b80e025fbc0b06745e218158656",
                },
            ],
        }),
        embedding: None,
        comes_with: None,
    },
    Model {
        name: "granite-4.2-8b",
        title: "IBM Granite 4.2 8B, Q4_K_M",
        repository: "ibm-granite/granite-4.2-8b-GGUF",
        revision: "93f3f6a8938ee922b784cf4e5b4203cd3428df8f",
        file: "granite-4.2-8b-Q4_K_M.gguf",
        size: 5_347_917_952,
        sha256: "16a9369d0805f80b7377d25d87f937a90c05dc04ad79173a52001e42c9aab311",
        license: "Apache-2.0",
        memory_gb: 16,
        recommended: false,
        mlx: Some(MlxBuild {
            repository: "ibm-granite/granite-4.2-8b-q4-mlx",
            revision: "9047f073e46c527f70dade07bb55b1700d728e36",
            files: &[
                ModelFile {
                    path: "chat_template.jinja",
                    size: 9_013,
                    sha256: "f0ba43f79b3cabca5e5a7584c77aec92f49feae45cdf7779c87d9fc54cd90258",
                },
                ModelFile {
                    path: "config.json",
                    size: 1_187,
                    sha256: "a1ee533ce70c3792b0281f9c7458ad4513ea3a9102e860f5da6f3f636e19c75c",
                },
                ModelFile {
                    path: "generation_config.json",
                    size: 207,
                    sha256: "1b9baf0a7959d31d7fb58ff6b506c27d09a73d395cadccc7e6dd1d4b7e43fd9d",
                },
                ModelFile {
                    path: "model.safetensors",
                    size: 4_945_853_970,
                    sha256: "86a0c04a5deb24de6ada66a37c7373e3e37dbf2a138ed28bb03c212d331b6429",
                },
                ModelFile {
                    path: "tokenizer.json",
                    size: 7_153_414,
                    sha256: "883975314d58743726347caaf2c32ac89afd5ad57222cffbc4f052f2ab27b2d7",
                },
                ModelFile {
                    path: "tokenizer_config.json",
                    size: 436,
                    sha256: "0b1cc678b9fbde8ce90c81e0f6483c4e1696dfa377330e42a88106416b96e659",
                },
            ],
        }),
        embedding: None,
        comes_with: None,
    },
    Model {
        name: "qwen3.5-9b",
        title: "Qwen3.5 9B, Q4_K_M",
        repository: "unsloth/Qwen3.5-9B-GGUF",
        revision: "3885219b6810b007914f3a7950a8d1b469d598a5",
        file: "Qwen3.5-9B-Q4_K_M.gguf",
        size: 5_680_522_464,
        sha256: "03b74727a860a56338e042c4420bb3f04b2fec5734175f4cb9fa853daf52b7e8",
        license: "Apache-2.0",
        memory_gb: 16,
        recommended: false,
        mlx: Some(MlxBuild {
            repository: "mlx-community/Qwen3.5-9B-MLX-4bit",
            revision: "938d8919941c6e7efd3c7150eff7fe9d12afa631",
            files: &[
                ModelFile {
                    path: "chat_template.jinja",
                    size: 7_756,
                    sha256: "a4aee8afcf2e0711942cf848899be66016f8d14a889ff9ede07bca099c28f715",
                },
                ModelFile {
                    path: "config.json",
                    size: 3_331,
                    sha256: "a96942cb6a8a1d3f1d17514d81a1925d04362a6a3233b389d13012211baaa9f8",
                },
                ModelFile {
                    path: "model-00001-of-00002.safetensors",
                    size: 5_349_771_222,
                    sha256: "a68b87558c6ef43f74c2bd63ce7e9092ceddc3101f3def0030774bae5f42aadd",
                },
                ModelFile {
                    path: "model-00002-of-00002.safetensors",
                    size: 600_449_850,
                    sha256: "b0a770bf8469c7f3f18756a0e0283f1c1174344a83e059a4e483f6af4907352d",
                },
                ModelFile {
                    path: "model.safetensors.index.json",
                    size: 123_592,
                    sha256: "dd023913fb87cfdae27fb11dcf695117c925833796ccac3c64117d6652d8ff1e",
                },
                ModelFile {
                    path: "tokenizer.json",
                    size: 19_989_343,
                    sha256: "87a7830d63fcf43bf241c3c5242e96e62dd3fdc29224ca26fed8ea333db72de4",
                },
                ModelFile {
                    path: "tokenizer_config.json",
                    size: 1_139,
                    sha256: "e98f1901ac6f0adff67b1d540bfa0c36ac1a0cf59eb72ed78146ef89aafa1182",
                },
            ],
        }),
        embedding: None,
        comes_with: None,
    },
    Model {
        name: "embeddinggemma-300m",
        title: "EmbeddingGemma 300M, Q8_0",
        repository: "ggml-org/embeddinggemma-300M-GGUF",
        revision: "0f741b5a6585bd53aeb15cd1372c56f2a0f65e12",
        file: "embeddinggemma-300M-Q8_0.gguf",
        size: 333_590_944,
        sha256: "b5ce9d77a3fc4b3b39ccb5643c36777911cc4eb46a66962eadfa3f5f60490d63",
        license: "Gemma",
        memory_gb: 2,
        recommended: false,
        mlx: Some(MlxBuild {
            repository: "mlx-community/embeddinggemma-300m-8bit",
            revision: "c8316c9e35cf8830541b04d315398c67faa6e497",
            files: &[
                ModelFile {
                    path: "config.json",
                    size: 1_726,
                    sha256: "c4498b5ab0058828f8c535fa4198a1ea117f6ad533c3d774d9f8784eb13b38cb",
                },
                ModelFile {
                    path: "model.safetensors",
                    size: 326_958_701,
                    sha256: "fcd54e677a802889db7cde3f4cce2e620c55b5239203379d8b7431e2252af286",
                },
                ModelFile {
                    path: "tokenizer.json",
                    size: 33_385_008,
                    sha256: "6852f8d561078cc0cebe70ca03c5bfdd0d60a45f9d2e0e1e4cc05b68e9ec329e",
                },
                ModelFile {
                    path: "tokenizer_config.json",
                    size: 1_155_346,
                    sha256: "9076840490613047bc9115963ee96b7702018b0d26ba644240bf856efda93118",
                },
            ],
        }),
        embedding: Some(Embedding {
            dimensions: 768,
            context_tokens: 2048,
            pooling: Pooling::Mean,
            query_prefix: "task: search result | query: ",
            document_prefix: "title: none | text: ",
        }),
        comes_with: None,
    },
    Model {
        name: "qwen3-embedding-0.6b",
        title: "Qwen3-Embedding 0.6B, Q8_0",
        repository: "Qwen/Qwen3-Embedding-0.6B-GGUF",
        revision: "370f27d7550e0def9b39c1f16d3fbaa13aa67728",
        file: "Qwen3-Embedding-0.6B-Q8_0.gguf",
        size: 639_150_592,
        sha256: "06507c7b42688469c4e7298b0a1e16deff06caf291cf0a5b278c308249c3e439",
        license: "Apache-2.0",
        memory_gb: 2,
        recommended: false,
        mlx: Some(MlxBuild {
            repository: "mlx-community/Qwen3-Embedding-0.6B-8bit",
            revision: "407ad2329cd30702720aafe83f74a1ba30fdfbca",
            files: &[
                ModelFile {
                    path: "config.json",
                    size: 857,
                    sha256: "65fb46306262bac47c6a89e5ad8a766c3d40d559057c1931e9c10976266a4fc7",
                },
                ModelFile {
                    path: "model.safetensors",
                    size: 633_152_041,
                    sha256: "fe956e8d346b4f08215a3cfc48a874354f900c20a59e965b75df0d9d77c54b28",
                },
                ModelFile {
                    path: "tokenizer.json",
                    size: 11_423_705,
                    sha256: "def76fb086971c7867b829c23a26261e38d9d74e02139253b38aeb9df8b4b50a",
                },
                ModelFile {
                    path: "tokenizer_config.json",
                    size: 5_404,
                    sha256: "443bfa629eb16387a12edbf92a76f6a6f10b2af3b53d87ba1550adfcf45f7fa0",
                },
            ],
        }),
        embedding: Some(Embedding {
            dimensions: 1024,
            context_tokens: 8192,
            pooling: Pooling::Last,
            query_prefix: "Instruct: Given a web search query, retrieve relevant passages that answer the query\nQuery:",
            document_prefix: "",
        }),
        comes_with: None,
    },
    Model {
        name: "apple-nl-sentence-en",
        title: "Apple sentence embedding, English (Apple devices only)",
        repository: "",
        revision: "",
        file: "",
        size: 0,
        sha256: "",
        license: "macOS",
        memory_gb: 1,
        recommended: false,
        mlx: None,
        embedding: Some(Embedding {
            dimensions: 512,
            context_tokens: 256,
            pooling: Pooling::Own,
            query_prefix: "",
            document_prefix: "",
        }),
        comes_with: Some("carrel-provider-apple"),
    },
];

/// The embedding model new archives use (`docs/design/search.md`).
pub const DEFAULT_EMBEDDING_MODEL: &str = "embeddinggemma-300m";

/// The catalog's embedding models, the default first.
pub fn embedding_models() -> impl Iterator<Item = &'static Model> {
    CATALOG.iter().filter(|m| m.embedding.is_some())
}

/// The catalog's text models.
pub fn text_models() -> impl Iterator<Item = &'static Model> {
    CATALOG.iter().filter(|m| m.embedding.is_none())
}

/// The hosts a catalog download may reach: Hugging Face, and its CDN, which it redirects to.
pub const ALLOWED_HOSTS: &[&str] = &["huggingface.co", ".hf.co"];

/// The catalog's model called `name`.
#[must_use]
pub fn find(name: &str) -> Option<&'static Model> {
    CATALOG.iter().find(|m| m.name == name)
}

/// What the network helper is asked to do, as one JSON value on its standard input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchRequest {
    /// The `https` URL to download.
    pub url: String,
    /// The hosts it and any redirect may be on. One starting with `.` allows any host ending
    /// with it.
    pub hosts: Vec<String>,
    /// The size the file must have, in bytes.
    pub size: u64,
    /// The SHA-256 it must have, in lowercase hexadecimal.
    pub sha256: String,
    /// The file to write. If it exists, the download continues from its end.
    pub to: PathBuf,
    /// The log to append a line to for every request made, if any.
    pub log: Option<PathBuf>,
}

/// What the network helper reports, one JSON value per line on its standard output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum FetchEvent {
    /// How much of the file is written.
    Progress {
        /// Bytes written so far, including any continued from an earlier attempt.
        done: u64,
        /// The file's full size.
        total: u64,
    },
    /// The file is complete, with the size and SHA-256 asked for.
    Done {
        /// Its size.
        bytes: u64,
        /// Its SHA-256.
        sha256: String,
    },
    /// The download stopped. A partial file is kept so it can continue, unless it was wrong.
    Failed {
        /// Why, in words.
        message: String,
    },
}

/// Whether `host` is allowed by `hosts`, as [`FetchRequest::hosts`] describes.
#[must_use]
pub fn host_allowed(host: &str, hosts: &[String]) -> bool {
    let host = host.to_ascii_lowercase();
    hosts.iter().any(|allowed| {
        let allowed = allowed.to_ascii_lowercase();
        match allowed.strip_prefix('.') {
            Some(domain) => host.ends_with(&allowed) && host.len() > domain.len() + 1,
            None => host == allowed,
        }
    })
}

/// Why a model couldn't be downloaded or removed.
#[derive(Debug)]
pub enum ModelError {
    /// No model in the catalog has this name.
    Unknown(String),
    /// There's no models folder: no configuration folder and no `CARREL_MODELS_DIR`.
    NoFolder,
    /// The `carrel-fetch` helper isn't installed.
    NoHelper,
    /// The helper reported a failure, or stopped without finishing.
    Fetch(String),
    /// The finished file didn't have the size or SHA-256 the catalog gives.
    Mismatch {
        /// What was expected.
        expected: String,
        /// What the file had.
        found: String,
    },
    /// The model isn't downloaded.
    NotDownloaded(String),
    /// The model has no MLX build in the catalog.
    NoMlx(String),
    /// The model's file isn't pinned to a size and SHA-256 yet, so it can't be checked.
    Unpinned(String),
    /// The model comes with a provider, the second field, so there's nothing to download.
    ComesWith(String, &'static str),
    /// Reading or writing the models folder failed.
    Io(std::io::Error),
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(name) => {
                write!(f, "no model called {name}; `carrel models` lists them")
            }
            Self::NoFolder => write!(f, "no models folder: set CARREL_MODELS_DIR"),
            Self::NoHelper => write!(
                f,
                "the network helper, carrel-fetch, isn't installed beside carrel or on PATH"
            ),
            Self::Fetch(message) => write!(f, "download failed: {message}"),
            Self::Mismatch { expected, found } => write!(
                f,
                "the downloaded file isn't the one expected ({expected}, found {found}), so it was deleted"
            ),
            Self::NotDownloaded(name) => write!(f, "{name} isn't downloaded"),
            Self::NoMlx(name) => write!(f, "{name} has no MLX build; download it as GGUF instead"),
            Self::Unpinned(name) => write!(
                f,
                "{name}'s file isn't pinned to a checksum yet, so it can't be downloaded safely"
            ),
            Self::ComesWith(name, provider) => write!(
                f,
                "{name} comes with {provider}, so there's nothing to download"
            ),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ModelError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ModelError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Something that downloads a file as a [`FetchRequest`] asks: the network helper, or a test
/// double.
pub trait Fetcher {
    /// Downloads, calling `progress` with each [`FetchEvent::Progress`].
    ///
    /// # Errors
    ///
    /// [`ModelError::Fetch`] with the helper's message.
    fn fetch(
        &self,
        request: &FetchRequest,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<(), ModelError>;
}

/// The network helper, `carrel-fetch`.
#[cfg(not(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
)))]
#[derive(Debug, Clone)]
pub struct Helper {
    program: PathBuf,
}

#[cfg(not(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
)))]
impl Helper {
    /// The helper beside `carrel` or on `PATH`.
    ///
    /// # Errors
    ///
    /// [`ModelError::NoHelper`] if it isn't found.
    pub fn find() -> Result<Self, ModelError> {
        crate::providers::process::find_program(
            "carrel-fetch",
            &crate::providers::process::default_folders(),
        )
        .map(Self::at)
        .ok_or(ModelError::NoHelper)
    }

    /// The helper at `program`.
    #[must_use]
    pub fn at(program: PathBuf) -> Self {
        Self { program }
    }
}

#[cfg(not(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
)))]
impl Fetcher for Helper {
    fn fetch(
        &self,
        request: &FetchRequest,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<(), ModelError> {
        use std::io::{BufRead as _, Write as _};
        use std::process::{Command, Stdio};

        let mut child = Command::new(&self.program)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            let line = serde_json::to_string(request).map_err(std::io::Error::other)?;
            writeln!(stdin, "{line}")?;
        }
        let mut outcome = Err(ModelError::Fetch(
            "the helper stopped without finishing".into(),
        ));
        if let Some(stdout) = child.stdout.take() {
            for line in std::io::BufReader::new(stdout).lines() {
                match serde_json::from_str(&line?) {
                    Ok(FetchEvent::Progress { done, total }) => progress(done, total),
                    Ok(FetchEvent::Done { .. }) => outcome = Ok(()),
                    Ok(FetchEvent::Failed { message }) => outcome = Err(ModelError::Fetch(message)),
                    Err(e) => outcome = Err(ModelError::Fetch(format!("unreadable report: {e}"))),
                }
            }
        }
        child.wait()?;
        outcome
    }
}

/// A file in the models folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Installed {
    /// Its name without `.gguf`, which the llama.cpp provider offers it as.
    pub name: String,
    /// Where it is.
    pub path: PathBuf,
    /// Its size in bytes.
    pub size: u64,
}

/// The `.gguf` files in `dir`, by name, whether from the catalog or added by hand. A missing
/// folder has none.
///
/// # Errors
///
/// [`std::io::Error`] if the folder can't be read.
pub fn installed(dir: &Path) -> std::io::Result<Vec<Installed>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("gguf"))
        {
            continue;
        }
        let metadata = entry.metadata()?;
        if !metadata.is_file() {
            continue;
        }
        if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
            found.push(Installed {
                name: name.to_owned(),
                path: path.clone(),
                size: metadata.len(),
            });
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

/// The folder MLX models are kept in, inside the models folder `dir`.
#[must_use]
pub fn mlx_dir(dir: &Path) -> PathBuf {
    dir.join("mlx")
}

/// The MLX models in `dir`'s `mlx` folder: folders with a `config.json`, whether from the
/// catalog or added by hand, by name. A partial download (`NAME.part`) isn't one.
///
/// # Errors
///
/// [`std::io::Error`] if the folder can't be read.
pub fn installed_mlx(dir: &Path) -> std::io::Result<Vec<Installed>> {
    let entries = match std::fs::read_dir(mlx_dir(dir)) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut found = Vec::new();
    for entry in entries {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let partial = Path::new(name)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("part"));
        if name.starts_with('.') || partial || !path.join("config.json").is_file() {
            continue;
        }
        let mut size = 0;
        for file in std::fs::read_dir(&path)? {
            let metadata = file?.metadata()?;
            if metadata.is_file() {
                size += metadata.len();
            }
        }
        found.push(Installed {
            name: name.to_owned(),
            path: path.clone(),
            size,
        });
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

/// Downloads `model`'s MLX build into `dir`'s `mlx` folder through `fetcher`, one file at a
/// time into `NAME.part`, continuing any partial file. Each file is checked against the
/// catalog as it finishes, and the folder is renamed to `NAME` only once every file is right,
/// so the provider never sees half a model. `progress` gets the bytes done of the whole.
///
/// # Errors
///
/// [`ModelError::NoMlx`] if the model has no MLX build, [`ModelError::Fetch`] if a download
/// stops, [`ModelError::Mismatch`] if a file isn't the one expected (it's deleted), or
/// [`ModelError::Io`].
pub fn download_mlx(
    model: &Model,
    dir: &Path,
    fetcher: &dyn Fetcher,
    log: Option<&Path>,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf, ModelError> {
    let build = model
        .mlx
        .ok_or_else(|| ModelError::NoMlx(model.name.to_owned()))?;
    let folder = mlx_dir(dir).join(model.name);
    if folder.is_dir() {
        return Ok(folder);
    }
    let part = mlx_dir(dir).join(format!("{}.part", model.name));
    std::fs::create_dir_all(&part)?;
    let total = build.size();
    let mut before = 0;
    for file in build.files {
        let to = part.join(file.path);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let finished = to.is_file() && hash_file(&to)? == (file.size, file.sha256.to_owned());
        if !finished {
            let request = FetchRequest {
                url: build.url(file),
                hosts: ALLOWED_HOSTS.iter().map(|h| (*h).to_owned()).collect(),
                size: file.size,
                sha256: file.sha256.to_owned(),
                to: to.clone(),
                log: log.map(Path::to_owned),
            };
            fetcher.fetch(&request, &mut |done, _| progress(before + done, total))?;
            let (size, sha256) = hash_file(&to)?;
            if size != file.size || sha256 != file.sha256 {
                std::fs::remove_file(&to)?;
                return Err(ModelError::Mismatch {
                    expected: format!(
                        "{}: {} bytes, SHA-256 {}",
                        file.path, file.size, file.sha256
                    ),
                    found: format!("{size} bytes, SHA-256 {sha256}"),
                });
            }
        }
        before += file.size;
        progress(before, total);
    }
    std::fs::rename(&part, &folder)?;
    Ok(folder)
}

/// Deletes the MLX model called `name` from `dir`'s `mlx` folder, and any partial download.
///
/// # Errors
///
/// [`ModelError::NotDownloaded`] if there's nothing to delete, or [`ModelError::Io`].
pub fn remove_mlx(name: &str, dir: &Path) -> Result<PathBuf, ModelError> {
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        return Err(ModelError::NotDownloaded(name.to_owned()));
    }
    let folder = mlx_dir(dir).join(name);
    let part = mlx_dir(dir).join(format!("{name}.part"));
    let had_part = part.is_dir();
    if had_part {
        std::fs::remove_dir_all(&part)?;
    }
    if folder.is_dir() {
        std::fs::remove_dir_all(&folder)?;
        Ok(folder)
    } else if had_part {
        Ok(part)
    } else {
        Err(ModelError::NotDownloaded(name.to_owned()))
    }
}

/// Where a model's partial download is kept.
#[must_use]
pub fn part_path(model: &Model, dir: &Path) -> PathBuf {
    dir.join(format!("{}.part", model.file_name()))
}

/// The device log every network request is recorded in.
#[must_use]
pub fn network_log() -> Option<PathBuf> {
    crate::local::config_dir().map(|dir| dir.join("logs").join("network.log"))
}

/// Downloads `model` into `dir` through `fetcher`, continuing a partial download if there is
/// one. The file is renamed to [`Model::file_name`] only once its size and SHA-256 match the
/// catalog's; if they don't, it's deleted.
///
/// # Errors
///
/// [`ModelError::ComesWith`] if a provider has it built in, [`ModelError::Unpinned`] if the
/// catalog doesn't pin its file yet, [`ModelError::Fetch`] if the download stops,
/// [`ModelError::Mismatch`] if the file isn't the one expected, or [`ModelError::Io`].
pub fn download(
    model: &Model,
    dir: &Path,
    fetcher: &dyn Fetcher,
    log: Option<&Path>,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf, ModelError> {
    if let Some(provider) = model.comes_with {
        return Err(ModelError::ComesWith(model.name.to_owned(), provider));
    }
    if !model.is_pinned() {
        return Err(ModelError::Unpinned(model.name.to_owned()));
    }
    std::fs::create_dir_all(dir)?;
    let part = part_path(model, dir);
    let request = FetchRequest {
        url: model.url(),
        hosts: ALLOWED_HOSTS.iter().map(|h| (*h).to_owned()).collect(),
        size: model.size,
        sha256: model.sha256.to_owned(),
        to: part.clone(),
        log: log.map(Path::to_owned),
    };
    fetcher.fetch(&request, progress)?;
    // The helper checked the file too, but it's a separate program: check it here as well.
    let (size, sha256) = hash_file(&part)?;
    if size != model.size || sha256 != model.sha256 {
        std::fs::remove_file(&part)?;
        return Err(ModelError::Mismatch {
            expected: format!("{} bytes, SHA-256 {}", model.size, model.sha256),
            found: format!("{size} bytes, SHA-256 {sha256}"),
        });
    }
    let path = dir.join(model.file_name());
    std::fs::rename(&part, &path)?;
    Ok(path)
}

/// Deletes the downloaded model called `name` from `dir`, and any partial download of it.
///
/// # Errors
///
/// [`ModelError::NotDownloaded`] if there's nothing to delete, or [`ModelError::Io`].
pub fn remove(name: &str, dir: &Path) -> Result<PathBuf, ModelError> {
    // Only a file directly in the folder: a name like `../x` mustn't reach outside it.
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        return Err(ModelError::NotDownloaded(name.to_owned()));
    }
    let path = dir.join(format!("{name}.gguf"));
    let part = dir.join(format!("{name}.gguf.part"));
    let had_part = part.is_file();
    if had_part {
        std::fs::remove_file(&part)?;
    }
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && had_part => Ok(part),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(ModelError::NotDownloaded(name.to_owned()))
        }
        Err(e) => Err(e.into()),
    }
}

/// A file's size and SHA-256, in lowercase hexadecimal.
///
/// # Errors
///
/// [`std::io::Error`] if it can't be read.
pub fn hash_file(path: &Path) -> std::io::Result<(u64, String)> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    let mut size = 0;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size += read as u64;
    }
    Ok((size, hex(&hasher.finalize())))
}

/// Bytes as lowercase hexadecimal.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes `contents` to the request's file, as a download would.
    struct Writes(Vec<u8>);

    impl Fetcher for Writes {
        fn fetch(
            &self,
            request: &FetchRequest,
            progress: &mut dyn FnMut(u64, u64),
        ) -> Result<(), ModelError> {
            std::fs::write(&request.to, &self.0)?;
            progress(self.0.len() as u64, request.size);
            Ok(())
        }
    }

    struct Fails;

    impl Fetcher for Fails {
        fn fetch(&self, _: &FetchRequest, _: &mut dyn FnMut(u64, u64)) -> Result<(), ModelError> {
            Err(ModelError::Fetch("connection reset".into()))
        }
    }

    fn model_of(contents: &[u8]) -> (Model, String) {
        let sha256 = hex(&Sha256::digest(contents));
        let model = Model {
            sha256: "",
            size: contents.len() as u64,
            ..CATALOG[0]
        };
        (model, sha256)
    }

    #[test]
    fn the_catalog_is_pinned_and_permissive() {
        for model in CATALOG {
            if let Some(provider) = model.comes_with {
                // A model a provider has built in has nothing to download or pin.
                assert!(provider.starts_with("carrel-provider-"), "{}", model.name);
                assert!(model.embedding.is_some(), "{}", model.name);
                assert_eq!(
                    (model.repository, model.file, model.revision, model.size),
                    ("", "", "", 0)
                );
                assert!(model.mlx.is_none());
                continue;
            }
            assert!(model.is_pinned(), "{} isn't pinned", model.name);
            assert_eq!(model.revision.len(), 40, "{}", model.name);
            assert_eq!(model.sha256.len(), 64, "{}", model.name);
            assert!(
                model
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            );
            // EmbeddingGemma, chosen in docs/design/search.md, is under Google's Gemma terms.
            assert!(
                ["Apache-2.0", "MIT"].contains(&model.license)
                    || (model.license == "Gemma" && model.embedding.is_some()),
                "{}",
                model.name
            );
            assert!(
                Path::new(model.file)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("gguf"))
            );
            assert!(model.url().starts_with("https://huggingface.co/"));
            assert!(crate::names::is_portable(&model.file_name()));
            if let Some(build) = model.mlx {
                assert_eq!(build.revision.len(), 40, "{}", model.name);
                assert!(build.files.iter().any(|f| f.path == "config.json"));
                assert!(build.files.iter().any(|f| f.path.ends_with(".safetensors")));
                for file in build.files {
                    assert_eq!(file.sha256.len(), 64, "{} {}", model.name, file.path);
                    assert!(!file.path.contains(".."), "{}", file.path);
                }
            }
        }
        let default = find(DEFAULT_EMBEDDING_MODEL).expect("the default is in the catalog");
        assert!(default.embedding.is_some());
        assert_eq!(embedding_models().next(), Some(default));
        assert_eq!(
            text_models().count() + embedding_models().count(),
            CATALOG.len()
        );
        let mut names: Vec<_> = CATALOG.iter().map(|m| m.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), CATALOG.len());
    }

    #[test]
    fn hosts_match_exactly_or_by_domain() {
        let hosts: Vec<String> = ALLOWED_HOSTS.iter().map(|h| (*h).to_owned()).collect();
        assert!(host_allowed("huggingface.co", &hosts));
        assert!(host_allowed("HuggingFace.co", &hosts));
        assert!(host_allowed("us.aws.cdn.hf.co", &hosts));
        assert!(!host_allowed("hf.co.evil.example", &hosts));
        assert!(!host_allowed("evilhf.co", &hosts));
        assert!(!host_allowed("huggingface.co.evil.example", &hosts));
        assert!(!host_allowed("example.com", &hosts));
    }

    #[test]
    fn a_checked_download_gets_its_name() {
        let dir = tempfile::tempdir().unwrap();
        let contents = b"GGUF model".to_vec();
        let (mut model, sha256) = model_of(&contents);
        let sha256: &'static str = Box::leak(sha256.into_boxed_str());
        model.sha256 = sha256;
        let mut seen = Vec::new();
        let path = download(&model, dir.path(), &Writes(contents), None, &mut |d, t| {
            seen.push((d, t));
        })
        .unwrap();
        assert_eq!(path, dir.path().join("granite-4.2-3b.gguf"));
        assert!(!part_path(&model, dir.path()).exists());
        assert_eq!(seen, [(10, 10)]);
        let found = installed(dir.path()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "granite-4.2-3b");
    }

    #[test]
    fn a_file_that_does_not_match_is_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let (mut model, _) = model_of(b"GGUF model");
        model.sha256 = "0000000000000000000000000000000000000000000000000000000000000000";
        let error = download(
            &model,
            dir.path(),
            &Writes(b"GGUF model".to_vec()),
            None,
            &mut |_, _| {},
        )
        .unwrap_err();
        assert!(matches!(error, ModelError::Mismatch { .. }), "{error}");
        assert_eq!(installed(dir.path()).unwrap(), []);
        assert!(!part_path(&model, dir.path()).exists());
    }

    #[test]
    fn an_unpinned_model_is_never_downloaded() {
        let dir = tempfile::tempdir().unwrap();
        let unpinned = Model {
            revision: "",
            sha256: "",
            size: 0,
            ..CATALOG[0]
        };
        let error = download(
            &unpinned,
            dir.path(),
            &Writes(b"anything".to_vec()),
            None,
            &mut |_, _| {},
        )
        .unwrap_err();
        assert!(matches!(error, ModelError::Unpinned(_)), "{error}");
        assert!(!part_path(&unpinned, dir.path()).exists());
    }

    #[test]
    fn a_model_that_comes_with_a_provider_is_never_downloaded() {
        let dir = tempfile::tempdir().unwrap();
        let apple = find("apple-nl-sentence-en").unwrap();
        let error = download(
            apple,
            dir.path(),
            &Writes(b"anything".to_vec()),
            None,
            &mut |_, _| {},
        )
        .unwrap_err();
        assert!(matches!(
            error,
            ModelError::ComesWith(_, "carrel-provider-apple")
        ));
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
    }

    /// The Swift providers' copies of the catalog's embedding models, which can't read this
    /// one: the MLX provider's table and Apple's sentence embedding.
    #[test]
    fn the_swift_providers_embed_as_the_catalog_says() {
        let apple = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apple");
        let read = |path: &str| std::fs::read_to_string(apple.join(path)).unwrap();
        let mlx = read("CarrelMLX/Sources/CarrelMLXProvider/EmbeddingCatalog.swift");
        let swift = |text: &str| text.replace('\n', "\\n");
        for model in embedding_models() {
            let embedding = model.embedding.unwrap();
            if model.comes_with.is_some() {
                let apple =
                    read("CarrelProviders/Sources/CarrelAppleProviders/SentenceEmbedding.swift");
                assert!(apple.contains(&format!("id = \"{}\"", model.name)));
                assert!(apple.contains(&format!("dimensions = {}", embedding.dimensions)));
                assert!(apple.contains(&format!("contextWords = {}", embedding.context_tokens)));
                continue;
            }
            let pooling = match embedding.pooling {
                Pooling::Mean => ".mean",
                Pooling::Last => ".last",
                Pooling::Own => unreachable!("{} pools itself", model.name),
            };
            let entry = format!(
                "id: \"{}\", label: \"{}\", dimensions: {},\n            contextTokens: {}, pooling: {pooling},",
                model.name,
                mlx_label(model),
                embedding.dimensions,
                embedding.context_tokens,
            );
            assert!(
                mlx.contains(&entry),
                "{} in the MLX provider: {entry}",
                model.name
            );
            for prefix in [embedding.query_prefix, embedding.document_prefix] {
                assert!(
                    mlx.contains(&format!("\"{}\"", swift(prefix))),
                    "{}'s prefix {prefix:?} in the MLX provider",
                    model.name
                );
            }
        }
    }

    /// The MLX provider's label for an embedding model: its title, without the GGUF's
    /// quantization.
    fn mlx_label(model: &Model) -> String {
        let name = model.title.split(',').next().unwrap_or(model.title);
        format!("{name} (MLX)")
    }

    #[test]
    fn embedding_prompts_say_what_the_text_is_for() {
        use crate::providers::Purpose;
        let gemma = find(DEFAULT_EMBEDDING_MODEL).unwrap().embedding.unwrap();
        assert_eq!(
            gemma.prompt("the lease", Purpose::Query),
            "task: search result | query: the lease"
        );
        assert_eq!(
            gemma.prompt("A lease.", Purpose::Document),
            "title: none | text: A lease."
        );
    }

    #[test]
    fn a_failed_download_names_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let error = download(&CATALOG[0], dir.path(), &Fails, None, &mut |_, _| {}).unwrap_err();
        assert!(error.to_string().contains("connection reset"));
        assert_eq!(installed(dir.path()).unwrap(), []);
    }

    #[test]
    fn removing_deletes_the_model_and_any_part() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.gguf"), b"x").unwrap();
        std::fs::write(dir.path().join("b.gguf.part"), b"x").unwrap();
        assert_eq!(remove("a", dir.path()).unwrap(), dir.path().join("a.gguf"));
        assert_eq!(
            remove("b", dir.path()).unwrap(),
            dir.path().join("b.gguf.part")
        );
        assert!(matches!(
            remove("c", dir.path()),
            Err(ModelError::NotDownloaded(_))
        ));
        assert_eq!(installed(dir.path()).unwrap(), []);
    }

    #[test]
    fn requests_and_events_round_trip_as_json() {
        let request = FetchRequest {
            url: "https://huggingface.co/x".into(),
            hosts: vec!["huggingface.co".into()],
            size: 3,
            sha256: "ab".into(),
            to: PathBuf::from("m.gguf.part"),
            log: None,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert_eq!(
            serde_json::from_str::<FetchRequest>(&json).unwrap(),
            request
        );
        let event = FetchEvent::Progress { done: 1, total: 3 };
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(json, r#"{"event":"progress","done":1,"total":3}"#);
        assert_eq!(serde_json::from_str::<FetchEvent>(&json).unwrap(), event);
    }

    /// Writes each file's contents, chosen by the end of its URL.
    struct Files(Vec<(&'static str, Vec<u8>)>);

    impl Fetcher for Files {
        fn fetch(
            &self,
            request: &FetchRequest,
            progress: &mut dyn FnMut(u64, u64),
        ) -> Result<(), ModelError> {
            let (_, contents) = self
                .0
                .iter()
                .find(|(path, _)| request.url.ends_with(path))
                .ok_or_else(|| ModelError::Fetch(format!("unexpected {}", request.url)))?;
            std::fs::write(&request.to, contents)?;
            progress(contents.len() as u64, request.size);
            Ok(())
        }
    }

    fn mlx_model(files: &[(&'static str, &[u8])]) -> Model {
        let files: Vec<ModelFile> = files
            .iter()
            .map(|(path, contents)| ModelFile {
                path,
                size: contents.len() as u64,
                sha256: Box::leak(hex(&Sha256::digest(contents)).into_boxed_str()),
            })
            .collect();
        Model {
            mlx: Some(MlxBuild {
                repository: "mlx-community/test",
                revision: "0000000000000000000000000000000000000000",
                files: Box::leak(files.into_boxed_slice()),
            }),
            ..CATALOG[0]
        }
    }

    #[test]
    fn an_mlx_build_downloads_as_a_folder_only_once_every_file_checks_out() {
        let dir = tempfile::tempdir().unwrap();
        let model = mlx_model(&[("config.json", b"{}"), ("model.safetensors", b"weights")]);
        let mut seen = Vec::new();
        let fetcher = Files(vec![
            ("config.json", b"{}".to_vec()),
            ("model.safetensors", b"weights".to_vec()),
        ]);
        let folder = download_mlx(&model, dir.path(), &fetcher, None, &mut |d, t| {
            seen.push((d, t));
        })
        .unwrap();
        assert_eq!(folder, dir.path().join("mlx").join(model.name));
        assert!(folder.join("model.safetensors").is_file());
        assert_eq!(
            seen.last(),
            Some(&(9, 9)),
            "progress is of the whole folder"
        );
        let listed = installed_mlx(dir.path()).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].size, 9);
        assert!(installed(dir.path()).unwrap().is_empty(), "not a GGUF file");

        remove_mlx(model.name, dir.path()).unwrap();
        assert_eq!(installed_mlx(dir.path()).unwrap(), Vec::new());
        assert!(matches!(
            remove_mlx(model.name, dir.path()),
            Err(ModelError::NotDownloaded(_))
        ));
    }

    #[test]
    fn a_wrong_file_is_deleted_and_the_folder_stays_partial() {
        let dir = tempfile::tempdir().unwrap();
        let model = mlx_model(&[("config.json", b"{}"), ("model.safetensors", b"weights")]);
        let wrong = Files(vec![
            ("config.json", b"{}".to_vec()),
            ("model.safetensors", b"tampered".to_vec()),
        ]);
        assert!(matches!(
            download_mlx(&model, dir.path(), &wrong, None, &mut |_, _| {}),
            Err(ModelError::Mismatch { .. })
        ));
        let part = dir.path().join("mlx").join(format!("{}.part", model.name));
        assert!(
            part.join("config.json").is_file(),
            "finished files are kept"
        );
        assert!(!part.join("model.safetensors").exists());
        assert_eq!(installed_mlx(dir.path()).unwrap(), Vec::new());

        // Carrying on fetches only what's missing.
        let rest = Files(vec![("model.safetensors", b"weights".to_vec())]);
        download_mlx(&model, dir.path(), &rest, None, &mut |_, _| {}).unwrap();
        assert_eq!(installed_mlx(dir.path()).unwrap().len(), 1);

        let none = Model { mlx: None, ..model };
        assert!(matches!(
            download_mlx(&none, dir.path(), &rest, None, &mut |_, _| {}),
            Err(ModelError::NoMlx(_))
        ));
    }
}
