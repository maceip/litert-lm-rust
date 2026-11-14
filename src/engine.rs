//! Engine builder and Engine handle.

use std::ffi::{CStr, CString};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;

use crate::conversation::{Conversation, ConversationConfig};
use crate::error::{check, Error, Result};
use crate::ffi;
use crate::session::{Session, SessionConfig};

/// Hardware backend for model execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Backend {
    Cpu,
    Gpu,
    /// Vendor / platform-specific backend string (e.g. `"npu"`, `"openvino"`).
    Custom(String),
}

impl Backend {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Cpu => "cpu",
            Self::Gpu => "gpu",
            Self::Custom(s) => s,
        }
    }
}

/// Activation data type for the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationDataType {
    Float32,
    Float16,
    Int16,
    Int8,
}

impl ActivationDataType {
    pub(crate) fn into_ffi(self) -> ffi::LiteRtLmActivationDataType {
        match self {
            Self::Float32 => ffi::LiteRtLmActivationDataType_kLiteRtLmActivationDataTypeFloat32,
            Self::Float16 => ffi::LiteRtLmActivationDataType_kLiteRtLmActivationDataTypeFloat16,
            Self::Int16 => ffi::LiteRtLmActivationDataType_kLiteRtLmActivationDataTypeInt16,
            Self::Int8 => ffi::LiteRtLmActivationDataType_kLiteRtLmActivationDataTypeInt8,
        }
    }
}

/// Speculative decoding (MTP) mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeculativeDecodingMode {
    Off,
    On,
    Adaptive,
}

impl SpeculativeDecodingMode {
    pub(crate) fn into_ffi(self) -> ffi::LiteRtLmSpeculativeDecodingMode {
        match self {
            Self::Off => ffi::LiteRtLmSpeculativeDecodingMode_kLiteRtLmSpeculativeDecodingModeOff,
            Self::On => ffi::LiteRtLmSpeculativeDecodingMode_kLiteRtLmSpeculativeDecodingModeOn,
            Self::Adaptive => {
                ffi::LiteRtLmSpeculativeDecodingMode_kLiteRtLmSpeculativeDecodingModeAdaptive
            }
        }
    }
}

/// Speculative decoding (MTP) configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeculativeDecodingConfig {
    pub mode: SpeculativeDecodingMode,
    pub lenience: Option<f32>,
}

impl SpeculativeDecodingConfig {
    pub fn new(mode: SpeculativeDecodingMode) -> Self {
        Self {
            mode,
            lenience: None,
        }
    }

    pub fn with_lenience(mut self, lenience: f32) -> Self {
        self.lenience = Some(lenience);
        self
    }

    pub(crate) fn create_ffi(self) -> Result<RawSpeculativeDecodingConfig> {
        let mut raw = std::ptr::null_mut();
        check(
            "litert_lm_speculative_decoding_config_create",
            unsafe { ffi::litert_lm_speculative_decoding_config_create(&mut raw) },
        )?;
        let raw = NonNull::new(raw).ok_or(Error::NullPointer(
            "litert_lm_speculative_decoding_config_create",
        ))?;
        unsafe {
            check(
                "litert_lm_speculative_decoding_config_set_mode",
                ffi::litert_lm_speculative_decoding_config_set_mode(raw.as_ptr(), self.mode.into_ffi()),
            )?;
            if let Some(v) = self.lenience {
                check(
                    "litert_lm_speculative_decoding_config_set_lenience",
                    ffi::litert_lm_speculative_decoding_config_set_lenience(raw.as_ptr(), v),
                )?;
            }
        }
        Ok(RawSpeculativeDecodingConfig { raw })
    }
}

pub(crate) struct RawSpeculativeDecodingConfig {
    raw: NonNull<ffi::LiteRtLmSpeculativeDecodingConfig>,
}

impl RawSpeculativeDecodingConfig {
    pub(crate) fn as_ptr(&self) -> *const ffi::LiteRtLmSpeculativeDecodingConfig {
        self.raw.as_ptr()
    }
}

impl Drop for RawSpeculativeDecodingConfig {
    fn drop(&mut self) {
        unsafe { ffi::litert_lm_speculative_decoding_config_delete(self.raw.as_ptr()) };
    }
}

/// Native library log severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogSeverity {
    Verbose,
    Debug,
    Info,
    Warning,
    Error,
    Fatal,
    Silent,
}

impl LogSeverity {
    pub(crate) fn into_ffi(self) -> ffi::LiteRtLmLogSeverity {
        match self {
            Self::Verbose => ffi::LiteRtLmLogSeverity_kLiteRtLmLogSeverityVerbose,
            Self::Debug => ffi::LiteRtLmLogSeverity_kLiteRtLmLogSeverityDebug,
            Self::Info => ffi::LiteRtLmLogSeverity_kLiteRtLmLogSeverityInfo,
            Self::Warning => ffi::LiteRtLmLogSeverity_kLiteRtLmLogSeverityWarning,
            Self::Error => ffi::LiteRtLmLogSeverity_kLiteRtLmLogSeverityError,
            Self::Fatal => ffi::LiteRtLmLogSeverity_kLiteRtLmLogSeverityFatal,
            Self::Silent => ffi::LiteRtLmLogSeverity_kLiteRtLmLogSeveritySilent,
        }
    }
}

/// Builder for [`Engine`].
///
/// Supports multimodal backends and MTP (Multi-Token Prediction).
#[derive(Debug, Clone)]
pub struct EngineBuilder {
    model_path: PathBuf,
    backend: Backend,
    vision_backend: Option<Backend>,
    audio_backend: Option<Backend>,
    max_num_tokens: Option<i32>,
    num_threads: Option<i32>,
    audio_num_threads: Option<i32>,
    parallel_file_section_loading: Option<bool>,
    single_threaded_execution: Option<bool>,
    cache_dir: Option<PathBuf>,
    prefill_chunk_size: Option<i32>,
    max_num_images: Option<i32>,
    max_vision_tokens_per_image: Option<i32>,
    dispatch_lib_dir: Option<PathBuf>,
    activation_data_type: Option<ActivationDataType>,
    /// Enables Multi-Token Prediction (MTP) via speculative decoding.
    multi_token_prediction: Option<bool>,
    speculative_decoding_config: Option<SpeculativeDecodingConfig>,
    enable_benchmark: bool,
    num_prefill_tokens: Option<i32>,
    num_decode_tokens: Option<i32>,
    gpu_decode_steps_per_sync: Option<i32>,
    gpu_wait_for_weight_uploads: Option<bool>,
    gpu_enable_metal_residency_set: Option<bool>,
    use_ringbuffers_local_attention: Option<bool>,
    enable_ynnpack: Option<bool>,
    selected_signatures: Option<Vec<String>>,
    lora_rank: Option<i32>,
    supported_lora_ranks: Option<Vec<i32>>,
    audio_lora_rank: Option<i32>,
    supported_audio_lora_ranks: Option<Vec<i32>>,
}

impl EngineBuilder {
    pub fn backend(mut self, backend: Backend) -> Self {
        self.backend = backend;
        self
    }

    /// Vision backend for multimodal image understanding.
    pub fn vision_backend(mut self, backend: Backend) -> Self {
        self.vision_backend = Some(backend);
        self
    }

    /// Audio backend for multimodal audio understanding.
    pub fn audio_backend(mut self, backend: Backend) -> Self {
        self.audio_backend = Some(backend);
        self
    }

    pub fn max_num_tokens(mut self, max_num_tokens: i32) -> Self {
        self.max_num_tokens = Some(max_num_tokens);
        self
    }

    pub fn num_threads(mut self, num_threads: i32) -> Self {
        self.num_threads = Some(num_threads);
        self
    }

    pub fn audio_num_threads(mut self, num_threads: i32) -> Self {
        self.audio_num_threads = Some(num_threads);
        self
    }

    pub fn parallel_file_section_loading(mut self, enabled: bool) -> Self {
        self.parallel_file_section_loading = Some(enabled);
        self
    }

    /// Force single-threaded execution across engine, sessions, and
    /// conversations.
    pub fn single_threaded_execution(mut self, enabled: bool) -> Self {
        self.single_threaded_execution = Some(enabled);
        self
    }

    pub fn cache_dir(mut self, cache_dir: impl AsRef<Path>) -> Self {
        self.cache_dir = Some(cache_dir.as_ref().to_path_buf());
        self
    }

    pub fn prefill_chunk_size(mut self, size: i32) -> Self {
        self.prefill_chunk_size = Some(size);
        self
    }

    pub fn max_num_images(mut self, max_num_images: i32) -> Self {
        self.max_num_images = Some(max_num_images);
        self
    }

    pub fn max_vision_tokens_per_image(mut self, n: i32) -> Self {
        self.max_vision_tokens_per_image = Some(n);
        self
    }

    pub fn dispatch_lib_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.dispatch_lib_dir = Some(dir.as_ref().to_path_buf());
        self
    }

    pub fn activation_data_type(mut self, ty: ActivationDataType) -> Self {
        self.activation_data_type = Some(ty);
        self
    }

    /// Enable **Multi-Token Prediction (MTP)** via speculative decoding.
    ///
    /// Universally recommended for GPU backends. Maps to
    /// `litert_lm_engine_settings_set_enable_speculative_decoding`.
    pub fn multi_token_prediction(mut self, enabled: bool) -> Self {
        self.multi_token_prediction = Some(enabled);
        self
    }

    /// Alias for [`Self::multi_token_prediction`].
    pub fn enable_speculative_decoding(self, enabled: bool) -> Self {
        self.multi_token_prediction(enabled)
    }

    /// Full speculative decoding configuration (mode + lenience).
    pub fn speculative_decoding_config(mut self, config: SpeculativeDecodingConfig) -> Self {
        self.speculative_decoding_config = Some(config);
        self
    }

    pub fn enable_benchmark(mut self) -> Self {
        self.enable_benchmark = true;
        self
    }

    pub fn num_prefill_tokens(mut self, n: i32) -> Self {
        self.num_prefill_tokens = Some(n);
        self
    }

    pub fn num_decode_tokens(mut self, n: i32) -> Self {
        self.num_decode_tokens = Some(n);
        self
    }

    pub fn gpu_decode_steps_per_sync(mut self, steps: i32) -> Self {
        self.gpu_decode_steps_per_sync = Some(steps);
        self
    }

    pub fn gpu_wait_for_weight_uploads(mut self, wait: bool) -> Self {
        self.gpu_wait_for_weight_uploads = Some(wait);
        self
    }

    /// Enable the Metal residency set on Apple GPU backends.
    pub fn gpu_enable_metal_residency_set(mut self, enabled: bool) -> Self {
        self.gpu_enable_metal_residency_set = Some(enabled);
        self
    }

    pub fn use_ringbuffers_local_attention(mut self, enabled: bool) -> Self {
        self.use_ringbuffers_local_attention = Some(enabled);
        self
    }

    /// Enable the XNNPACK delegate.
    pub fn enable_ynnpack(mut self, enabled: bool) -> Self {
        self.enable_ynnpack = Some(enabled);
        self
    }

    /// Restrict the engine to a subset of the model's signatures.
    pub fn selected_signatures(mut self, signatures: impl Into<Vec<String>>) -> Self {
        self.selected_signatures = Some(signatures.into());
        self
    }

    pub fn lora_rank(mut self, rank: i32) -> Self {
        self.lora_rank = Some(rank);
        self
    }

    pub fn supported_lora_ranks(mut self, ranks: impl Into<Vec<i32>>) -> Self {
        self.supported_lora_ranks = Some(ranks.into());
        self
    }

    pub fn audio_lora_rank(mut self, rank: i32) -> Self {
        self.audio_lora_rank = Some(rank);
        self
    }

    pub fn supported_audio_lora_ranks(mut self, ranks: impl Into<Vec<i32>>) -> Self {
        self.supported_audio_lora_ranks = Some(ranks.into());
        self
    }

    /// Create the heavyweight [`Engine`] (loads model weights).
    pub fn build(self) -> Result<Engine> {
        let model_path = path_to_cstring(&self.model_path)?;
        let backend = CString::new(self.backend.as_str())?;
        let vision = optional_backend(&self.vision_backend)?;
        let audio = optional_backend(&self.audio_backend)?;

        let mut settings = std::ptr::null_mut();
        check(
            "litert_lm_engine_settings_create",
            unsafe {
                ffi::litert_lm_engine_settings_create(
                    model_path.as_ptr(),
                    backend.as_ptr(),
                    opt_cstr(&vision),
                    opt_cstr(&audio),
                    &mut settings,
                )
            },
        )?;
        let settings = NonNull::new(settings)
            .ok_or(Error::NullPointer("litert_lm_engine_settings_create"))?;
        let settings = RawEngineSettings { raw: settings };

        unsafe {
            if let Some(v) = self.max_num_tokens {
                check(
                    "litert_lm_engine_settings_set_max_num_tokens",
                    ffi::litert_lm_engine_settings_set_max_num_tokens(settings.raw.as_ptr(), v),
                )?;
            }
            if let Some(v) = self.num_threads {
                check(
                    "litert_lm_engine_settings_set_num_threads",
                    ffi::litert_lm_engine_settings_set_num_threads(settings.raw.as_ptr(), v),
                )?;
            }
            if let Some(v) = self.audio_num_threads {
                check(
                    "litert_lm_engine_settings_set_audio_num_threads",
                    ffi::litert_lm_engine_settings_set_audio_num_threads(settings.raw.as_ptr(), v),
                )?;
            }
            if let Some(v) = self.parallel_file_section_loading {
                check(
                    "litert_lm_engine_settings_set_parallel_file_section_loading",
                    ffi::litert_lm_engine_settings_set_parallel_file_section_loading(
                        settings.raw.as_ptr(),
                        v,
                    ),
                )?;
            }
            if let Some(v) = self.single_threaded_execution {
                check(
                    "litert_lm_engine_settings_set_single_threaded_execution",
                    ffi::litert_lm_engine_settings_set_single_threaded_execution(
                        settings.raw.as_ptr(),
                        v,
                    ),
                )?;
            }
            if let Some(ref dir) = self.cache_dir {
                let c = path_to_cstring(dir)?;
                check(
                    "litert_lm_engine_settings_set_cache_dir",
                    ffi::litert_lm_engine_settings_set_cache_dir(settings.raw.as_ptr(), c.as_ptr()),
                )?;
            }
            if let Some(v) = self.prefill_chunk_size {
                check(
                    "litert_lm_engine_settings_set_prefill_chunk_size",
                    ffi::litert_lm_engine_settings_set_prefill_chunk_size(
                        settings.raw.as_ptr(),
                        v,
                    ),
                )?;
            }
            if let Some(v) = self.max_num_images {
                check(
                    "litert_lm_engine_settings_set_max_num_images",
                    ffi::litert_lm_engine_settings_set_max_num_images(settings.raw.as_ptr(), v),
                )?;
            }
            if let Some(v) = self.max_vision_tokens_per_image {
                check(
                    "litert_lm_engine_settings_set_max_vision_tokens_per_image",
                    ffi::litert_lm_engine_settings_set_max_vision_tokens_per_image(
                        settings.raw.as_ptr(),
                        v,
                    ),
                )?;
            }
            if let Some(ref dir) = self.dispatch_lib_dir {
                let c = path_to_cstring(dir)?;
                check(
                    "litert_lm_engine_settings_set_litert_dispatch_lib_dir",
                    ffi::litert_lm_engine_settings_set_litert_dispatch_lib_dir(
                        settings.raw.as_ptr(),
                        c.as_ptr(),
                    ),
                )?;
            }
            if let Some(ty) = self.activation_data_type {
                check(
                    "litert_lm_engine_settings_set_activation_data_type",
                    ffi::litert_lm_engine_settings_set_activation_data_type(
                        settings.raw.as_ptr(),
                        ty.into_ffi(),
                    ),
                )?;
            }
            if let Some(enabled) = self.multi_token_prediction {
                check(
                    "litert_lm_engine_settings_set_enable_speculative_decoding",
                    ffi::litert_lm_engine_settings_set_enable_speculative_decoding(
                        settings.raw.as_ptr(),
                        enabled,
                    ),
                )?;
            }
            if let Some(cfg) = self.speculative_decoding_config {
                let raw_cfg = cfg.create_ffi()?;
                check(
                    "litert_lm_engine_settings_set_speculative_decoding_config",
                    ffi::litert_lm_engine_settings_set_speculative_decoding_config(
                        settings.raw.as_ptr(),
                        raw_cfg.as_ptr(),
                    ),
                )?;
            }
            if self.enable_benchmark {
                check(
                    "litert_lm_engine_settings_enable_benchmark",
                    ffi::litert_lm_engine_settings_enable_benchmark(settings.raw.as_ptr()),
                )?;
            }
            if let Some(v) = self.num_prefill_tokens {
                check(
                    "litert_lm_engine_settings_set_num_prefill_tokens",
                    ffi::litert_lm_engine_settings_set_num_prefill_tokens(
                        settings.raw.as_ptr(),
                        v,
                    ),
                )?;
            }
            if let Some(v) = self.num_decode_tokens {
                check(
                    "litert_lm_engine_settings_set_num_decode_tokens",
                    ffi::litert_lm_engine_settings_set_num_decode_tokens(settings.raw.as_ptr(), v),
                )?;
            }
            if let Some(v) = self.gpu_decode_steps_per_sync {
                check(
                    "litert_lm_engine_settings_set_gpu_decode_steps_per_sync",
                    ffi::litert_lm_engine_settings_set_gpu_decode_steps_per_sync(
                        settings.raw.as_ptr(),
                        v,
                    ),
                )?;
            }
            if let Some(v) = self.gpu_wait_for_weight_uploads {
                check(
                    "litert_lm_engine_settings_set_gpu_wait_for_weight_uploads",
                    ffi::litert_lm_engine_settings_set_gpu_wait_for_weight_uploads(
                        settings.raw.as_ptr(),
                        v,
                    ),
                )?;
            }
            if let Some(v) = self.gpu_enable_metal_residency_set {
                check(
                    "litert_lm_engine_settings_set_gpu_enable_metal_residency_set",
                    ffi::litert_lm_engine_settings_set_gpu_enable_metal_residency_set(
                        settings.raw.as_ptr(),
                        v,
                    ),
                )?;
            }
            if let Some(v) = self.use_ringbuffers_local_attention {
                check(
                    "litert_lm_engine_settings_set_use_ringbuffers_local_attention",
                    ffi::litert_lm_engine_settings_set_use_ringbuffers_local_attention(
                        settings.raw.as_ptr(),
                        v,
                    ),
                )?;
            }
            if let Some(v) = self.enable_ynnpack {
                check(
                    "litert_lm_engine_settings_set_enable_ynnpack",
                    ffi::litert_lm_engine_settings_set_enable_ynnpack(settings.raw.as_ptr(), v),
                )?;
            }
            if let Some(ref signatures) = self.selected_signatures {
                let c_strings: Vec<CString> = signatures
                    .iter()
                    .map(|s| CString::new(s.as_str()))
                    .collect::<std::result::Result<_, _>>()?;
                let ptrs: Vec<*const std::os::raw::c_char> =
                    c_strings.iter().map(|c| c.as_ptr()).collect();
                check(
                    "litert_lm_engine_settings_set_selected_signatures",
                    ffi::litert_lm_engine_settings_set_selected_signatures(
                        settings.raw.as_ptr(),
                        ptrs.as_ptr(),
                        ptrs.len(),
                    ),
                )?;
            }
            if let Some(v) = self.lora_rank {
                check(
                    "litert_lm_engine_settings_set_lora_rank",
                    ffi::litert_lm_engine_settings_set_lora_rank(settings.raw.as_ptr(), v),
                )?;
            }
            if let Some(ref ranks) = self.supported_lora_ranks {
                check(
                    "litert_lm_engine_settings_set_supported_lora_ranks",
                    ffi::litert_lm_engine_settings_set_supported_lora_ranks(
                        settings.raw.as_ptr(),
                        ranks.as_ptr(),
                        ranks.len(),
                    ),
                )?;
            }
            if let Some(v) = self.audio_lora_rank {
                check(
                    "litert_lm_engine_settings_set_audio_lora_rank",
                    ffi::litert_lm_engine_settings_set_audio_lora_rank(settings.raw.as_ptr(), v),
                )?;
            }
            if let Some(ref ranks) = self.supported_audio_lora_ranks {
                check(
                    "litert_lm_engine_settings_set_supported_audio_lora_ranks",
                    ffi::litert_lm_engine_settings_set_supported_audio_lora_ranks(
                        settings.raw.as_ptr(),
                        ranks.as_ptr(),
                        ranks.len(),
                    ),
                )?;
            }
        }

        let mut engine = std::ptr::null_mut();
        check(
            "litert_lm_engine_create",
            unsafe { ffi::litert_lm_engine_create(settings.raw.as_ptr(), &mut engine) },
        )?;
        let raw = NonNull::new(engine).ok_or(Error::NullPointer("litert_lm_engine_create"))?;
        Ok(Engine { raw })
    }
}

/// Heavyweight model holder. Create lightweight [`Conversation`] / [`Session`] from it.
pub struct Engine {
    pub(crate) raw: NonNull<ffi::LiteRtLmEngine>,
}

// Engine owns unique native state; Send is OK if the native lib allows cross-thread
// handoff of the opaque pointer (documented as OK to create sessions from one engine).
unsafe impl Send for Engine {}
unsafe impl Sync for Engine {}

impl Engine {
    pub fn builder(model_path: impl AsRef<Path>) -> EngineBuilder {
        EngineBuilder {
            model_path: model_path.as_ref().to_path_buf(),
            backend: Backend::Cpu,
            vision_backend: None,
            audio_backend: None,
            max_num_tokens: None,
            num_threads: None,
            audio_num_threads: None,
            parallel_file_section_loading: None,
            single_threaded_execution: None,
            cache_dir: None,
            prefill_chunk_size: None,
            max_num_images: None,
            max_vision_tokens_per_image: None,
            dispatch_lib_dir: None,
            activation_data_type: None,
            multi_token_prediction: None,
            speculative_decoding_config: None,
            enable_benchmark: false,
            num_prefill_tokens: None,
            num_decode_tokens: None,
            gpu_decode_steps_per_sync: None,
            gpu_wait_for_weight_uploads: None,
            gpu_enable_metal_residency_set: None,
            use_ringbuffers_local_attention: None,
            enable_ynnpack: None,
            selected_signatures: None,
            lora_rank: None,
            supported_lora_ranks: None,
            audio_lora_rank: None,
            supported_audio_lora_ranks: None,
        }
    }

    pub fn create_session(&self, config: SessionConfig) -> Result<Session<'_>> {
        Session::new(self, config)
    }

    pub fn create_conversation(&self, config: ConversationConfig) -> Result<Conversation<'_>> {
        Conversation::new(self, config)
    }

    /// Tokenize UTF-8 text with the engine tokenizer.
    pub fn tokenize(&self, text: &str) -> Result<Vec<i32>> {
        let c_text = CString::new(text)?;
        let mut result = std::ptr::null_mut();
        check(
            "litert_lm_engine_tokenize",
            unsafe {
                ffi::litert_lm_engine_tokenize(self.raw.as_ptr(), c_text.as_ptr(), &mut result)
            },
        )?;
        let result =
            NonNull::new(result).ok_or(Error::NullPointer("litert_lm_engine_tokenize"))?;
        let tokens = unsafe {
            let mut n = 0;
            check(
                "litert_lm_tokenize_result_get_num_tokens",
                ffi::litert_lm_tokenize_result_get_num_tokens(result.as_ptr(), &mut n),
            )?;
            let mut ptr: *const std::os::raw::c_int = std::ptr::null();
            check(
                "litert_lm_tokenize_result_get_tokens",
                ffi::litert_lm_tokenize_result_get_tokens(result.as_ptr(), &mut ptr),
            )?;
            if ptr.is_null() || n == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(ptr, n).to_vec()
            }
        };
        unsafe { ffi::litert_lm_tokenize_result_delete(result.as_ptr()) };
        Ok(tokens)
    }

    /// Detokenize token ids to a UTF-8 string.
    pub fn detokenize(&self, tokens: &[i32]) -> Result<String> {
        let mut result = std::ptr::null_mut();
        check(
            "litert_lm_engine_detokenize",
            unsafe {
                ffi::litert_lm_engine_detokenize(
                    self.raw.as_ptr(),
                    tokens.as_ptr(),
                    tokens.len(),
                    &mut result,
                )
            },
        )?;
        let result =
            NonNull::new(result).ok_or(Error::NullPointer("litert_lm_engine_detokenize"))?;
        let s = unsafe {
            let mut ptr: *const std::os::raw::c_char = std::ptr::null();
            check(
                "litert_lm_detokenize_result_get_string",
                ffi::litert_lm_detokenize_result_get_string(result.as_ptr(), &mut ptr),
            )?;
            if ptr.is_null() {
                String::new()
            } else {
                CStr::from_ptr(ptr).to_str()?.to_owned()
            }
        };
        unsafe { ffi::litert_lm_detokenize_result_delete(result.as_ptr()) };
        Ok(s)
    }

    pub(crate) fn raw(&self) -> NonNull<ffi::LiteRtLmEngine> {
        self.raw
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        unsafe { ffi::litert_lm_engine_delete(self.raw.as_ptr()) };
    }
}

struct RawEngineSettings {
    raw: NonNull<ffi::LiteRtLmEngineSettings>,
}

impl Drop for RawEngineSettings {
    fn drop(&mut self) {
        unsafe { ffi::litert_lm_engine_settings_delete(self.raw.as_ptr()) };
    }
}

pub(crate) fn path_to_cstring(path: &Path) -> Result<CString> {
    let s = path.to_str().ok_or(Error::ModelPath)?;
    Ok(CString::new(s)?)
}

fn optional_backend(backend: &Option<Backend>) -> Result<Option<CString>> {
    backend
        .as_ref()
        .map(|b| CString::new(b.as_str()))
        .transpose()
        .map_err(Into::into)
}

fn opt_cstr(value: &Option<CString>) -> *const std::os::raw::c_char {
    value
        .as_ref()
        .map(|c| c.as_ptr())
        .unwrap_or(std::ptr::null())
}

/// Marker used by child objects that borrow an [`Engine`].
pub(crate) type EngineLifetime<'a> = PhantomData<&'a Engine>;
