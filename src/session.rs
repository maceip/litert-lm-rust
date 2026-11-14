//! Low-level Session API (prefill / decode / generate_content).

use std::ffi::{CStr, CString};
use std::ptr::NonNull;

use crate::engine::{Engine, EngineLifetime};
use crate::error::{check, Error, Result};
use crate::ffi;
use crate::input::{InputData, OwnedInputs};
use crate::sampler::SamplerParams;
use crate::stream::{self, StreamEventReceiver};

/// Configuration for a [`Session`].
#[derive(Debug, Clone, Default)]
pub struct SessionConfig {
    pub max_output_tokens: Option<i32>,
    pub apply_prompt_template: Option<bool>,
    pub sampler: Option<SamplerParams>,
    pub lora_path: Option<String>,
    pub audio_lora_path: Option<String>,
}

/// Stateful generation session owned by an [`Engine`].
pub struct Session<'engine> {
    raw: NonNull<ffi::LiteRtLmSession>,
    _engine: EngineLifetime<'engine>,
}

unsafe impl Send for Session<'_> {}

impl<'engine> Session<'engine> {
    pub(crate) fn new(engine: &'engine Engine, config: SessionConfig) -> Result<Self> {
        let raw_config = RawSessionConfig::new(config)?;
        let mut raw = std::ptr::null_mut();
        check(
            "litert_lm_engine_create_session",
            unsafe {
                ffi::litert_lm_engine_create_session(
                    engine.raw().as_ptr(),
                    raw_config.as_mut_ptr(),
                    &mut raw,
                )
            },
        )?;
        let raw = NonNull::new(raw).ok_or(Error::NullPointer("litert_lm_engine_create_session"))?;
        Ok(Self {
            raw,
            _engine: EngineLifetime::default(),
        })
    }

    /// Clone this session (copied KV cache / state).
    pub fn clone_session(&self) -> Result<Session<'engine>> {
        let mut raw = std::ptr::null_mut();
        check(
            "litert_lm_session_clone",
            unsafe { ffi::litert_lm_session_clone(self.raw.as_ptr(), &mut raw) },
        )?;
        let raw = NonNull::new(raw).ok_or(Error::NullPointer("litert_lm_session_clone"))?;
        Ok(Session {
            raw,
            _engine: EngineLifetime::default(),
        })
    }

    /// Save a labeled checkpoint of the session state.
    pub fn save_checkpoint(&mut self, label: &str) -> Result<()> {
        let c = CString::new(label)?;
        check(
            "litert_lm_session_save_checkpoint",
            unsafe { ffi::litert_lm_session_save_checkpoint(self.raw.as_ptr(), c.as_ptr()) },
        )
    }

    /// Rewind the session to a previously saved checkpoint.
    pub fn rewind_to_checkpoint(&mut self, label: &str) -> Result<()> {
        let c = CString::new(label)?;
        check(
            "litert_lm_session_rewind_to_checkpoint",
            unsafe { ffi::litert_lm_session_rewind_to_checkpoint(self.raw.as_ptr(), c.as_ptr()) },
        )
    }

    /// Rewind the session to a given step index.
    pub fn rewind_to_step(&mut self, step: i32) -> Result<()> {
        check(
            "litert_lm_session_rewind_to_step",
            unsafe { ffi::litert_lm_session_rewind_to_step(self.raw.as_ptr(), step) },
        )
    }

    pub fn generate_text(&mut self, prompt: &str) -> Result<String> {
        self.generate_content(&[InputData::Text(prompt.to_owned())])
    }

    pub fn generate_content(&mut self, inputs: &[InputData]) -> Result<String> {
        let responses = self.generate_content_responses(inputs)?;
        responses.text_at(0)
    }

    pub fn generate_content_responses(&mut self, inputs: &[InputData]) -> Result<Responses> {
        let owned = OwnedInputs::new(inputs)?;
        let mut raw = std::ptr::null_mut();
        check(
            "litert_lm_session_generate_content",
            unsafe {
                ffi::litert_lm_session_generate_content(
                    self.raw.as_ptr(),
                    owned.as_ptr(),
                    owned.len(),
                    &mut raw,
                )
            },
        )?;
        let raw =
            NonNull::new(raw).ok_or(Error::NullPointer("litert_lm_session_generate_content"))?;
        Ok(Responses { raw })
    }

    /// Stream multimodal generation. Callbacks arrive on a background thread.
    pub fn generate_content_stream(
        &mut self,
        inputs: &[InputData],
    ) -> Result<StreamEventReceiver> {
        let owned = OwnedInputs::new(inputs)?;
        stream::start_session_stream(self.raw, owned)
    }

    pub fn run_prefill(&mut self, inputs: &[InputData]) -> Result<()> {
        let owned = OwnedInputs::new(inputs)?;
        check(
            "litert_lm_session_run_prefill",
            unsafe {
                ffi::litert_lm_session_run_prefill(self.raw.as_ptr(), owned.as_ptr(), owned.len())
            },
        )
    }

    pub fn run_decode(&mut self) -> Result<Responses> {
        let mut raw = std::ptr::null_mut();
        check(
            "litert_lm_session_run_decode",
            unsafe { ffi::litert_lm_session_run_decode(self.raw.as_ptr(), &mut raw) },
        )?;
        let raw = NonNull::new(raw).ok_or(Error::NullPointer("litert_lm_session_run_decode"))?;
        Ok(Responses { raw })
    }

    /// Score candidate target texts against the current session state.
    ///
    /// When `store_token_lengths` is true, [`Responses::token_length_at`] is
    /// populated for each candidate.
    pub fn run_text_scoring(
        &mut self,
        targets: &[&str],
        store_token_lengths: bool,
    ) -> Result<Responses> {
        let c_strings: Vec<CString> = targets
            .iter()
            .map(|t| CString::new(*t))
            .collect::<std::result::Result<_, _>>()?;
        let ptrs: Vec<*const std::os::raw::c_char> =
            c_strings.iter().map(|c| c.as_ptr()).collect();
        let mut raw = std::ptr::null_mut();
        check(
            "litert_lm_session_run_text_scoring",
            unsafe {
                ffi::litert_lm_session_run_text_scoring(
                    self.raw.as_ptr(),
                    ptrs.as_ptr() as *mut *const std::os::raw::c_char,
                    ptrs.len(),
                    store_token_lengths,
                    &mut raw,
                )
            },
        )?;
        let raw =
            NonNull::new(raw).ok_or(Error::NullPointer("litert_lm_session_run_text_scoring"))?;
        Ok(Responses { raw })
    }

    pub fn run_decode_async(&mut self) -> Result<StreamEventReceiver> {
        stream::start_decode_stream(self.raw)
    }

    pub fn cancel_process(&mut self) -> Result<()> {
        check(
            "litert_lm_session_cancel_process",
            unsafe { ffi::litert_lm_session_cancel_process(self.raw.as_ptr()) },
        )
    }

    pub fn benchmark_info(&self) -> Result<crate::BenchmarkInfo> {
        let mut raw = std::ptr::null_mut();
        check(
            "litert_lm_session_get_benchmark_info",
            unsafe {
                ffi::litert_lm_session_get_benchmark_info(self.raw.as_ptr(), &mut raw)
            },
        )?;
        let raw =
            NonNull::new(raw).ok_or(Error::NullPointer("litert_lm_session_get_benchmark_info"))?;
        Ok(crate::BenchmarkInfo { raw })
    }

    pub(crate) fn raw(&self) -> NonNull<ffi::LiteRtLmSession> {
        self.raw
    }
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        unsafe { ffi::litert_lm_session_delete(self.raw.as_ptr()) };
    }
}

/// Model responses from a blocking generate / decode call.
pub struct Responses {
    raw: NonNull<ffi::LiteRtLmResponses>,
}

impl Responses {
    pub fn num_candidates(&self) -> Result<usize> {
        let mut out = 0;
        check(
            "litert_lm_responses_get_num_candidates",
            unsafe {
                ffi::litert_lm_responses_get_num_candidates(self.raw.as_ptr(), &mut out)
            },
        )?;
        Ok(out)
    }

    pub fn text_at(&self, index: usize) -> Result<String> {
        let count = self.num_candidates()?;
        if index >= count {
            return Err(Error::IndexOutOfRange(index, count));
        }
        let mut ptr: *const std::os::raw::c_char = std::ptr::null();
        check(
            "litert_lm_responses_get_response_text_at",
            unsafe {
                ffi::litert_lm_responses_get_response_text_at(self.raw.as_ptr(), index, &mut ptr)
            },
        )?;
        if ptr.is_null() {
            return Err(Error::NullPointer(
                "litert_lm_responses_get_response_text_at",
            ));
        }
        let text = unsafe { CStr::from_ptr(ptr) }.to_str()?.to_owned();
        Ok(text)
    }

    pub fn score_at(&self, index: usize) -> Result<Option<f32>> {
        let mut has = false;
        check(
            "litert_lm_responses_has_score_at",
            unsafe {
                ffi::litert_lm_responses_has_score_at(self.raw.as_ptr(), index, &mut has)
            },
        )?;
        if !has {
            return Ok(None);
        }
        let mut out = 0.0;
        check(
            "litert_lm_responses_get_score_at",
            unsafe { ffi::litert_lm_responses_get_score_at(self.raw.as_ptr(), index, &mut out) },
        )?;
        Ok(Some(out))
    }

    pub fn token_length_at(&self, index: usize) -> Result<Option<usize>> {
        let mut has = false;
        check(
            "litert_lm_responses_has_token_length_at",
            unsafe {
                ffi::litert_lm_responses_has_token_length_at(self.raw.as_ptr(), index, &mut has)
            },
        )?;
        if !has {
            return Ok(None);
        }
        let mut out = 0;
        check(
            "litert_lm_responses_get_token_length_at",
            unsafe {
                ffi::litert_lm_responses_get_token_length_at(self.raw.as_ptr(), index, &mut out)
            },
        )?;
        Ok(Some(out))
    }

    pub fn token_scores_at(&self, index: usize) -> Result<Option<Vec<f32>>> {
        let mut has = false;
        check(
            "litert_lm_responses_has_token_scores_at",
            unsafe {
                ffi::litert_lm_responses_has_token_scores_at(self.raw.as_ptr(), index, &mut has)
            },
        )?;
        if !has {
            return Ok(None);
        }
        let mut n = 0;
        check(
            "litert_lm_responses_get_num_token_scores_at",
            unsafe {
                ffi::litert_lm_responses_get_num_token_scores_at(
                    self.raw.as_ptr(),
                    index,
                    &mut n,
                )
            },
        )?;
        let mut ptr: *const f32 = std::ptr::null();
        check(
            "litert_lm_responses_get_token_scores_at",
            unsafe {
                ffi::litert_lm_responses_get_token_scores_at(
                    self.raw.as_ptr(),
                    index,
                    &mut ptr,
                )
            },
        )?;
        if ptr.is_null() || n == 0 {
            return Ok(None);
        }
        Ok(Some(unsafe { std::slice::from_raw_parts(ptr, n) }.to_vec()))
    }
}

impl Drop for Responses {
    fn drop(&mut self) {
        unsafe { ffi::litert_lm_responses_delete(self.raw.as_ptr()) };
    }
}

pub(crate) struct RawSessionConfig {
    raw: Option<NonNull<ffi::LiteRtLmSessionConfig>>,
}

impl RawSessionConfig {
    pub(crate) fn new(config: SessionConfig) -> Result<Self> {
        let is_default = config.max_output_tokens.is_none()
            && config.apply_prompt_template.is_none()
            && config.sampler.is_none()
            && config.lora_path.is_none()
            && config.audio_lora_path.is_none();
        if is_default {
            return Ok(Self { raw: None });
        }

        let mut raw = std::ptr::null_mut();
        check(
            "litert_lm_session_config_create",
            unsafe { ffi::litert_lm_session_config_create(&mut raw) },
        )?;
        let raw =
            NonNull::new(raw).ok_or(Error::NullPointer("litert_lm_session_config_create"))?;
        let wrapper = Self { raw: Some(raw) };

        unsafe {
            if let Some(v) = config.max_output_tokens {
                check(
                    "litert_lm_session_config_set_max_output_tokens",
                    ffi::litert_lm_session_config_set_max_output_tokens(raw.as_ptr(), v),
                )?;
            }
            if let Some(v) = config.apply_prompt_template {
                check(
                    "litert_lm_session_config_set_apply_prompt_template",
                    ffi::litert_lm_session_config_set_apply_prompt_template(raw.as_ptr(), v),
                )?;
            }
            if let Some(sampler) = config.sampler {
                let params = sampler.create_ffi()?;
                check(
                    "litert_lm_session_config_set_sampler_params",
                    ffi::litert_lm_session_config_set_sampler_params(
                        raw.as_ptr(),
                        params.as_ptr(),
                    ),
                )?;
                // params dropped after call; C API copies values.
            }
            if let Some(ref path) = config.lora_path {
                let c = CString::new(path.as_str())?;
                check(
                    "litert_lm_session_config_set_lora_path",
                    ffi::litert_lm_session_config_set_lora_path(raw.as_ptr(), c.as_ptr()),
                )?;
            }
            if let Some(ref path) = config.audio_lora_path {
                let c = CString::new(path.as_str())?;
                check(
                    "litert_lm_session_config_set_audio_lora_path",
                    ffi::litert_lm_session_config_set_audio_lora_path(raw.as_ptr(), c.as_ptr()),
                )?;
            }
        }

        Ok(wrapper)
    }

    pub(crate) fn as_mut_ptr(&self) -> *mut ffi::LiteRtLmSessionConfig {
        self.raw.map(|p| p.as_ptr()).unwrap_or(std::ptr::null_mut())
    }

    pub(crate) fn as_ptr(&self) -> Option<NonNull<ffi::LiteRtLmSessionConfig>> {
        self.raw
    }
}

impl Drop for RawSessionConfig {
    fn drop(&mut self) {
        if let Some(raw) = self.raw.take() {
            unsafe { ffi::litert_lm_session_config_delete(raw.as_ptr()) };
        }
    }
}
