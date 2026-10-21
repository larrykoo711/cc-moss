//! 请求结果扫描独立于名称游标；库外锁不阻塞hook写入。

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use moss_core::daemon::lock;
use moss_core::registry::Registry;
use moss_core::slot::Status;
use moss_core::transcript::{request_file_stamp, request_result_for_session};
use serde_json::{json, Value};

pub fn refresh(registry: &Registry, dir: &Path) -> Result<()> {
    if registry.read_only() || registry.newer_schema().is_some() {
        return Ok(());
    }
    let Some(_lock) = lock::try_acquire(&dir.join("request-scan.lock"))? else {
        return Ok(());
    };
    let path = dir.join("request-scan.json");
    let mut cache: HashMap<String, Value> = std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let before = cache.clone();
    let slots = registry.slots()?;
    cache.retain(|id, _| {
        slots
            .iter()
            .any(|s| &s.slot_id == id && s.status != Status::Closed)
    });
    for slot in slots {
        if slot.status == Status::Closed {
            continue;
        }
        let Some(transcript) = slot.transcript_path.as_deref() else {
            continue;
        };
        let file = Path::new(transcript);
        let Ok(stamp) = request_file_stamp(file) else {
            continue;
        };
        let epoch = super::provider::evidence_epoch(&slot);
        let observed_ms = moss_core::slot::now_ms();
        let old = cache
            .get(&slot.slot_id)
            .filter(|v| v["session"] == slot.session_id && v["path"] == transcript);
        let unchanged = old.is_some_and(|v| v["stamp"] == stamp);
        if unchanged
            && old.is_some_and(|v| {
                v["config_epoch"].as_str() == epoch.as_deref() && v["fence_ms"].as_i64().is_some()
            })
        {
            continue;
        }
        let parts: Vec<_> = stamp.split(':').collect();
        let identity = parts[..2].join(":");
        let size: u64 = parts[2].parse()?;
        let growing = old.is_some_and(|v| {
            v["identity"] == identity
                && v["size"].as_u64().is_some_and(|n| n < size)
                && v["offset"]
                    .as_u64()
                    .and_then(|offset| boundary_fingerprint(file, offset).ok())
                    .is_some_and(|fingerprint| v["boundary"] == fingerprint)
        });
        let contiguous = unchanged || growing;
        let from = if contiguous {
            old.and_then(|v| v["offset"].as_u64()).unwrap_or(0)
        } else {
            0
        };
        let generation = if contiguous {
            old.and_then(|v| v["generation"].as_str()).unwrap_or(&stamp)
        } else {
            &stamp
        };
        let same_epoch = contiguous
            && old.is_some_and(|v| {
                v["config_epoch"].as_str() == epoch.as_deref()
                    && v["fence_ms"].as_i64().is_some()
                    && v["watermark"].as_u64().is_some()
            });
        let fence_ms = if same_epoch {
            old.and_then(|v| v["fence_ms"].as_i64())
                .unwrap_or(observed_ms)
        } else {
            observed_ms
        };
        if !same_epoch {
            registry.invalidate_model_evidence(&slot.slot_id, &slot.session_id)?;
        }
        let (result, to, boundary) = if unchanged {
            // 只有配置变化时复用已知完整行边界，不重新读取未变化的历史正文。
            (
                None,
                from,
                old.and_then(|v| v["boundary"].as_str())
                    .unwrap_or("")
                    .to_owned(),
            )
        } else {
            let Ok((result, to)) = request_result_for_session(file, from, Some(&slot.session_id))
            else {
                continue;
            };
            let Ok(boundary) = boundary_fingerprint(file, to) else {
                continue;
            };
            (result, to, boundary)
        };
        let watermark = if same_epoch {
            old.and_then(|v| v["watermark"].as_u64()).unwrap_or(to)
        } else {
            to
        };
        let scanned_ms = moss_core::slot::now_ms();
        if request_file_stamp(file).ok().as_deref() != Some(&stamp)
            || super::provider::evidence_epoch(&slot) != epoch
        {
            continue;
        }
        if let Some(mut result) = result {
            if same_epoch && result.position >= watermark {
                if let (Some(epoch), Some(success)) = (epoch.as_ref(), result.success.as_ref()) {
                    if let Some(at_ms) = success
                        .at_ms
                        .filter(|at| *at > fence_ms && *at <= scanned_ms)
                    {
                        if super::provider::model_plan(&slot)
                            .ok()
                            .and_then(|(plan, _)| plan.model)
                            .as_deref()
                            == Some(success.model.as_str())
                            && super::provider::evidence_epoch(&slot).as_ref() == Some(epoch)
                        {
                            result.evidence = Some(moss_core::transcript::ModelEvidence {
                                session_id: slot.session_id.clone(),
                                path: transcript.to_owned(),
                                generation: generation.to_owned(),
                                epoch: epoch.clone(),
                                fence_ms,
                                position: result.position,
                                model: success.model.clone(),
                                at_ms,
                            });
                        }
                    }
                }
            }
            if !registry.apply_api_result(
                &slot.slot_id,
                &slot.session_id,
                transcript,
                generation,
                &stamp,
                &result,
            )? {
                continue;
            }
        }
        cache.insert(
            slot.slot_id,
            json!({"session":slot.session_id,"path":transcript,
            "identity":identity,"stamp":stamp,"size":size,"offset":to,"generation":generation,"boundary":boundary,"config_epoch":epoch,"fence_ms":fence_ms,"watermark":watermark}),
        );
    }
    if cache != before {
        // 缓存写失败可重扫；主库中的位置与去重仍保证不会重复发事件。
        let tmp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec(&cache)?;
        if let Err(error) = std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, &path))
        {
            eprintln!("cc-moss: request scan cache: {error}");
        }
    }
    Ok(())
}

/// 只保存摘要，不把正文写入缓存。首部与完整行边界各最多4KiB，
/// 防止同inode截断后重长被误认为纯append；未变化文件不调用本函数。
fn boundary_fingerprint(path: &Path, offset: u64) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    for start in [0, offset.saturating_sub(4096)] {
        let mut bytes = vec![0; (offset - start).min(4096) as usize];
        file.seek(SeekFrom::Start(start))?;
        file.read_exact(&mut bytes)?;
        bytes.hash(&mut hash);
    }
    Ok(format!("{:016x}", hash.finish()))
}

/// 单一后台线程，避免JSONL读取占用100ms守护循环。
pub fn worker(stop: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            let result = Registry::open(&moss_core::store_dir::registry_path())
                .and_then(|registry| refresh(&registry, &moss_core::store_dir::state_dir()));
            if let Err(error) = result {
                eprintln!("cc-moss: request scan: {error}");
            }
            std::thread::park_timeout(Duration::from_secs(2));
        }
    })
}
