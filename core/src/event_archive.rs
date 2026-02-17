//! Local Crystal Ball event archive (JSONL).

use crate::crystal_ball::CrystalBallEvent;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    cmp::Ordering,
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

const GENESIS_HASH: &str = "GENESIS";
type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArchivedRecord {
    version: u8,
    prev_hash: String,
    hash: String,
    #[serde(default)]
    mac: Option<String>,
    event: CrystalBallEvent,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArchiveIntegrityReport {
    pub valid: bool,
    pub total_records: usize,
    pub signed_records: usize,
    pub legacy_unsigned_records: usize,
    pub hmac_configured: bool,
    pub mac_verified_records: usize,
    pub mac_missing_records: usize,
    pub mac_unverified_records: usize,
    pub first_invalid_line: Option<usize>,
    pub reason: Option<String>,
    pub last_hash: String,
}

enum ParsedArchiveLine {
    Signed(ArchivedRecord),
    Legacy(CrystalBallEvent),
}

#[derive(Debug, Clone)]
pub struct EventArchive {
    path: PathBuf,
    archive_ttl_secs: f64,
    hmac_key: Option<Vec<u8>>,
    lock: Arc<Mutex<()>>,
}

impl EventArchive {
    pub fn from_env() -> Self {
        let path = std::env::var("CRYSTAL_BALL_ARCHIVE_PATH")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("../data/crystal_ball_events.jsonl"));

        let ttl_days = std::env::var("CRYSTAL_BALL_ARCHIVE_TTL_DAYS")
            .ok()
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| *value > 0.0)
            .unwrap_or(30.0);

        let hmac_key = std::env::var("CRYSTAL_BALL_ARCHIVE_HMAC_KEY")
            .ok()
            .map(|value| value.trim().as_bytes().to_vec())
            .filter(|value| !value.is_empty());

        Self {
            path,
            archive_ttl_secs: ttl_days * 24.0 * 3600.0,
            hmac_key,
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn archive_ttl_days(&self) -> f64 {
        self.archive_ttl_secs / (24.0 * 3600.0)
    }

    pub fn hmac_configured(&self) -> bool {
        self.hmac_key.is_some()
    }

    pub fn verify_integrity(&self) -> Result<ArchiveIntegrityReport, String> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "archive lock poisoned".to_string())?;

        self.verify_integrity_unlocked()
    }

    pub fn load_recent(
        &self,
        window_secs: f64,
        limit: usize,
    ) -> Result<Vec<CrystalBallEvent>, String> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "archive lock poisoned".to_string())?;

        self.load_recent_unlocked(window_secs, limit)
    }

    pub fn append(&self, event: &CrystalBallEvent) -> Result<(), String> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "archive lock poisoned".to_string())?;

        ensure_parent_dir(self.path.as_path())?;

        let report = self.verify_integrity_unlocked()?;
        if !report.valid {
            return Err(format!(
                "Archive integrity check failed before append at line {:?}: {}",
                report.first_invalid_line,
                report
                    .reason
                    .unwrap_or_else(|| "unknown integrity error".to_string())
            ));
        }
        let record = sign_event(report.last_hash.as_str(), event, self.hmac_key.as_deref())?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path.as_path())
            .map_err(|err| format!("Failed to open archive file: {err}"))?;

        let line = serde_json::to_string(&record)
            .map_err(|err| format!("Failed to serialize archive record: {err}"))?;

        file.write_all(line.as_bytes())
            .map_err(|err| format!("Failed to write archive event: {err}"))?;
        file.write_all(b"\n")
            .map_err(|err| format!("Failed to write archive newline: {err}"))?;
        file.flush()
            .map_err(|err| format!("Failed to flush archive file: {err}"))?;

        Ok(())
    }

    pub fn compact(&self) -> Result<usize, String> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "archive lock poisoned".to_string())?;

        if !self.path.exists() {
            return Ok(0);
        }

        let keep = self.load_recent_unlocked(self.archive_ttl_secs, usize::MAX)?;

        ensure_parent_dir(self.path.as_path())?;
        let tmp_path = self.path.with_extension("jsonl.tmp");

        {
            let mut tmp_file = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(tmp_path.as_path())
                .map_err(|err| format!("Failed to open compact tmp file: {err}"))?;

            let mut prev_hash = GENESIS_HASH.to_string();
            for event in &keep {
                let record = sign_event(prev_hash.as_str(), event, self.hmac_key.as_deref())?;
                prev_hash = record.hash.clone();

                let line = serde_json::to_string(&record)
                    .map_err(|err| format!("Failed to serialize compact record: {err}"))?;
                tmp_file
                    .write_all(line.as_bytes())
                    .map_err(|err| format!("Failed to write compact record: {err}"))?;
                tmp_file
                    .write_all(b"\n")
                    .map_err(|err| format!("Failed to write compact newline: {err}"))?;
            }

            tmp_file
                .flush()
                .map_err(|err| format!("Failed to flush compact tmp file: {err}"))?;
        }

        fs::rename(tmp_path.as_path(), self.path.as_path())
            .map_err(|err| format!("Failed to replace archive file: {err}"))?;

        Ok(keep.len())
    }

    fn load_recent_unlocked(
        &self,
        window_secs: f64,
        limit: usize,
    ) -> Result<Vec<CrystalBallEvent>, String> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let now = now_secs();
        let cutoff = now - window_secs;

        let file = OpenOptions::new()
            .read(true)
            .open(self.path.as_path())
            .map_err(|err| format!("Failed to open archive for read: {err}"))?;

        let reader = BufReader::new(file);
        let mut events = Vec::new();

        for line in reader.lines() {
            let line = line.map_err(|err| format!("Failed reading archive line: {err}"))?;
            if line.trim().is_empty() {
                continue;
            }

            let event = match parse_archive_line(line.as_str()) {
                Ok(ParsedArchiveLine::Signed(record)) => record.event,
                Ok(ParsedArchiveLine::Legacy(event)) => event,
                Err(_) => {
                    continue;
                }
            };

            let Some(ts) = parse_ts(event.timestamp.as_str()) else {
                continue;
            };

            if ts >= cutoff {
                events.push(event);
            }
        }

        events.sort_by(|a, b| {
            let ta = parse_ts(a.timestamp.as_str()).unwrap_or(0.0);
            let tb = parse_ts(b.timestamp.as_str()).unwrap_or(0.0);
            ta.partial_cmp(&tb).unwrap_or(Ordering::Equal)
        });

        if limit != usize::MAX && events.len() > limit {
            let start = events.len() - limit;
            Ok(events[start..].to_vec())
        } else {
            Ok(events)
        }
    }

    fn verify_integrity_unlocked(&self) -> Result<ArchiveIntegrityReport, String> {
        let mut report = ArchiveIntegrityReport {
            valid: true,
            total_records: 0,
            signed_records: 0,
            legacy_unsigned_records: 0,
            hmac_configured: self.hmac_key.is_some(),
            mac_verified_records: 0,
            mac_missing_records: 0,
            mac_unverified_records: 0,
            first_invalid_line: None,
            reason: None,
            last_hash: GENESIS_HASH.to_string(),
        };

        if !self.path.exists() {
            return Ok(report);
        }

        let file = OpenOptions::new()
            .read(true)
            .open(self.path.as_path())
            .map_err(|err| format!("Failed to open archive for integrity check: {err}"))?;

        let reader = BufReader::new(file);
        let mut prev_hash = GENESIS_HASH.to_string();

        for (index, line) in reader.lines().enumerate() {
            let line_no = index + 1;
            let line = line.map_err(|err| format!("Failed reading archive line: {err}"))?;
            if line.trim().is_empty() {
                continue;
            }

            report.total_records += 1;
            match parse_archive_line(line.as_str()) {
                Ok(ParsedArchiveLine::Signed(record)) => {
                    report.signed_records += 1;

                    if record.prev_hash != prev_hash {
                        report.valid = false;
                        report.first_invalid_line = Some(line_no);
                        report.reason = Some(format!(
                            "prev_hash mismatch: expected {}, found {}",
                            prev_hash, record.prev_hash
                        ));
                        return Ok(report);
                    }

                    let expected_hash = compute_hash(record.prev_hash.as_str(), &record.event)?;
                    if record.hash != expected_hash {
                        report.valid = false;
                        report.first_invalid_line = Some(line_no);
                        report.reason = Some("record hash mismatch".to_string());
                        return Ok(report);
                    }

                    match (self.hmac_key.as_deref(), record.mac.as_deref()) {
                        (Some(key), Some(mac)) => {
                            let expected_mac = compute_mac(
                                key,
                                record.prev_hash.as_str(),
                                record.hash.as_str(),
                                &record.event,
                            )?;
                            if mac != expected_mac {
                                report.valid = false;
                                report.first_invalid_line = Some(line_no);
                                report.reason = Some("record mac mismatch".to_string());
                                return Ok(report);
                            }
                            report.mac_verified_records += 1;
                        }
                        (Some(_), None) => {
                            report.mac_missing_records += 1;
                        }
                        (None, Some(_)) => {
                            report.mac_unverified_records += 1;
                        }
                        (None, None) => {
                            report.mac_missing_records += 1;
                        }
                    }

                    prev_hash = record.hash;
                }
                Ok(ParsedArchiveLine::Legacy(event)) => {
                    report.legacy_unsigned_records += 1;
                    prev_hash = compute_hash(prev_hash.as_str(), &event)?;
                }
                Err(err) => {
                    report.valid = false;
                    report.first_invalid_line = Some(line_no);
                    report.reason = Some(err);
                    return Ok(report);
                }
            }
        }

        report.last_hash = prev_hash;
        Ok(report)
    }
}

fn parse_archive_line(line: &str) -> Result<ParsedArchiveLine, String> {
    if let Ok(record) = serde_json::from_str::<ArchivedRecord>(line) {
        return Ok(ParsedArchiveLine::Signed(record));
    }

    if let Ok(event) = serde_json::from_str::<CrystalBallEvent>(line) {
        return Ok(ParsedArchiveLine::Legacy(event));
    }

    Err("Line is neither a signed record nor a legacy event".to_string())
}

fn sign_event(
    prev_hash: &str,
    event: &CrystalBallEvent,
    hmac_key: Option<&[u8]>,
) -> Result<ArchivedRecord, String> {
    let hash = compute_hash(prev_hash, event)?;

    let mac = if let Some(key) = hmac_key {
        Some(compute_mac(key, prev_hash, hash.as_str(), event)?)
    } else {
        None
    };

    Ok(ArchivedRecord {
        version: if mac.is_some() { 2 } else { 1 },
        prev_hash: prev_hash.to_string(),
        hash,
        mac,
        event: event.clone(),
    })
}

fn compute_hash(prev_hash: &str, event: &CrystalBallEvent) -> Result<String, String> {
    let canonical = serde_json::to_string(event)
        .map_err(|err| format!("Failed to canonicalize event for hashing: {err}"))?;

    let mut hasher = Sha256::new();
    hasher.update(prev_hash.as_bytes());
    hasher.update(b"|");
    hasher.update(canonical.as_bytes());
    Ok(hex::encode(hasher.finalize()))
}

fn compute_mac(
    key: &[u8],
    prev_hash: &str,
    hash: &str,
    event: &CrystalBallEvent,
) -> Result<String, String> {
    let canonical = serde_json::to_string(event)
        .map_err(|err| format!("Failed to canonicalize event for MAC: {err}"))?;

    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|err| format!("Failed to initialize HMAC key: {err}"))?;
    mac.update(prev_hash.as_bytes());
    mac.update(b"|");
    mac.update(hash.as_bytes());
    mac.update(b"|");
    mac.update(canonical.as_bytes());

    Ok(hex::encode(mac.finalize().into_bytes()))
}

fn ensure_parent_dir(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("Failed to create archive directory: {err}"))?;
    }
    Ok(())
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn parse_ts(value: &str) -> Option<f64> {
    value.parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, sync::Arc, time::SystemTime};

    fn test_archive_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("{name}-{nanos}.jsonl"))
    }

    fn sample_event(id: &str, ts: &str, message: &str) -> CrystalBallEvent {
        CrystalBallEvent {
            event_id: id.to_string(),
            timestamp: ts.to_string(),
            event_type: "gate.transition".to_string(),
            source_actor: "Kaizen".to_string(),
            source_agent_id: "kaizen".to_string(),
            target_actor: "operator".to_string(),
            target_agent_id: "human".to_string(),
            task_id: "task-1".to_string(),
            message: message.to_string(),
            visibility: "operator".to_string(),
        }
    }

    fn test_archive(path: PathBuf) -> EventArchive {
        EventArchive {
            path,
            archive_ttl_secs: 365.0 * 24.0 * 3600.0,
            hmac_key: None,
            lock: Arc::new(Mutex::new(())),
        }
    }

    fn test_archive_with_hmac(path: PathBuf, key: &str) -> EventArchive {
        EventArchive {
            path,
            archive_ttl_secs: 365.0 * 24.0 * 3600.0,
            hmac_key: Some(key.as_bytes().to_vec()),
            lock: Arc::new(Mutex::new(())),
        }
    }

    #[test]
    fn append_and_verify_signed_chain() {
        let path = test_archive_path("archive-signed");
        let archive = test_archive(path.clone());

        archive
            .append(&sample_event("e-1", "1000.0", "hello"))
            .unwrap();
        archive
            .append(&sample_event("e-2", "1001.0", "world"))
            .unwrap();

        let report = archive.verify_integrity().unwrap();
        assert!(report.valid);
        assert_eq!(report.total_records, 2);
        assert_eq!(report.signed_records, 2);
        assert_eq!(report.legacy_unsigned_records, 0);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn detects_tampered_record() {
        let path = test_archive_path("archive-tamper");
        let archive = test_archive(path.clone());

        archive
            .append(&sample_event("e-1", "1000.0", "original"))
            .unwrap();

        let raw = fs::read_to_string(path.as_path()).unwrap();
        let mut lines: Vec<String> = raw.lines().map(|line| line.to_string()).collect();
        let mut record: ArchivedRecord = serde_json::from_str(lines[0].as_str()).unwrap();
        record.event.message = "tampered".to_string();
        lines[0] = serde_json::to_string(&record).unwrap();
        fs::write(path.as_path(), format!("{}\n", lines.join("\n"))).unwrap();

        let report = archive.verify_integrity().unwrap();
        assert!(!report.valid);
        assert_eq!(report.first_invalid_line, Some(1));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn compact_rewrites_legacy_records_as_signed() {
        let path = test_archive_path("archive-compact");
        let archive = test_archive(path.clone());

        let now = now_secs();
        let e1 = sample_event("e-1", format!("{:.3}", now - 2.0).as_str(), "legacy one");
        let e2 = sample_event("e-2", format!("{:.3}", now - 1.0).as_str(), "legacy two");
        let legacy = format!(
            "{}\n{}\n",
            serde_json::to_string(&e1).unwrap(),
            serde_json::to_string(&e2).unwrap()
        );
        fs::write(path.as_path(), legacy).unwrap();

        archive.compact().unwrap();
        let report = archive.verify_integrity().unwrap();

        assert!(report.valid);
        assert_eq!(report.total_records, 2);
        assert_eq!(report.signed_records, 2);
        assert_eq!(report.legacy_unsigned_records, 0);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn append_with_hmac_verifies_macs() {
        let path = test_archive_path("archive-hmac");
        let archive = test_archive_with_hmac(path.clone(), "test-hmac-key");

        archive
            .append(&sample_event("e-1", "1000.0", "secure"))
            .unwrap();
        archive
            .append(&sample_event("e-2", "1001.0", "secure two"))
            .unwrap();

        let report = archive.verify_integrity().unwrap();
        assert!(report.valid);
        assert!(report.hmac_configured);
        assert_eq!(report.mac_verified_records, 2);
        assert_eq!(report.mac_missing_records, 0);

        let _ = fs::remove_file(path);
    }
}
