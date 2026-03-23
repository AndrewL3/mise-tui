use std::collections::VecDeque;

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes == 0 {
        return "0 B".to_string();
    }
    let mut value = bytes as f64;
    for &unit in UNITS {
        if value < 1024.0 || unit == "TiB" {
            if unit == "B" {
                return format!("{value} {unit}");
            }
            return format!("{value:.1} {unit}");
        }
        value /= 1024.0;
    }
    unreachable!()
}

pub fn format_throughput(bytes_per_sec: f64, unit: &str) -> String {
    match unit {
        "KB/s" => format!("{:.1} KB/s", bytes_per_sec / 1_000.0),
        "MB/s" => format!("{:.1} MB/s", bytes_per_sec / 1_000_000.0),
        _ => {
            // "auto"
            if bytes_per_sec < 1_000.0 {
                format!("{} B/s", bytes_per_sec as u64)
            } else if bytes_per_sec < 1_000_000.0 {
                format!("{:.1} KB/s", bytes_per_sec / 1_000.0)
            } else {
                format!("{:.1} MB/s", bytes_per_sec / 1_000_000.0)
            }
        }
    }
}

pub fn push_capped(buf: &mut VecDeque<u64>, val: u64, cap: usize) {
    if cap == 0 {
        return;
    }
    buf.push_back(val);
    while buf.len() > cap {
        buf.pop_front();
    }
}

pub fn push_capped_f64(buf: &mut VecDeque<f64>, val: f64, cap: usize) {
    if cap == 0 {
        return;
    }
    buf.push_back(val);
    while buf.len() > cap {
        buf.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_zero() {
        assert_eq!(format_bytes(0), "0 B");
    }

    #[test]
    fn format_bytes_bytes_range() {
        assert_eq!(format_bytes(512), "512 B");
    }

    #[test]
    fn format_bytes_kib() {
        assert_eq!(format_bytes(1536), "1.5 KiB");
    }

    #[test]
    fn format_bytes_mib() {
        assert_eq!(format_bytes(882_900_992), "842.0 MiB");
    }

    #[test]
    fn format_bytes_gib() {
        assert_eq!(format_bytes(1_610_612_736), "1.5 GiB");
    }

    #[test]
    fn format_bytes_tib() {
        assert_eq!(format_bytes(2_199_023_255_552), "2.0 TiB");
    }

    #[test]
    fn format_throughput_auto_bytes() {
        assert_eq!(format_throughput(500.0, "auto"), "500 B/s");
    }

    #[test]
    fn format_throughput_auto_kb() {
        assert_eq!(format_throughput(1500.0, "auto"), "1.5 KB/s");
    }

    #[test]
    fn format_throughput_auto_mb() {
        assert_eq!(format_throughput(1_500_000.0, "auto"), "1.5 MB/s");
    }

    #[test]
    fn format_throughput_explicit_kb() {
        assert_eq!(format_throughput(1_500_000.0, "KB/s"), "1500.0 KB/s");
    }

    #[test]
    fn format_throughput_explicit_mb() {
        assert_eq!(format_throughput(1_500_000.0, "MB/s"), "1.5 MB/s");
    }

    #[test]
    fn format_throughput_zero() {
        assert_eq!(format_throughput(0.0, "auto"), "0 B/s");
    }

    #[test]
    fn push_capped_within_cap() {
        let mut buf = VecDeque::new();
        push_capped(&mut buf, 1, 3);
        push_capped(&mut buf, 2, 3);
        assert_eq!(buf.len(), 2);
        assert_eq!(buf[0], 1);
        assert_eq!(buf[1], 2);
    }

    #[test]
    fn push_capped_at_cap_evicts_front() {
        let mut buf = VecDeque::from(vec![1, 2, 3]);
        push_capped(&mut buf, 4, 3);
        assert_eq!(buf.len(), 3);
        assert_eq!(buf[0], 2);
        assert_eq!(buf[2], 4);
    }

    #[test]
    fn push_capped_zero_cap_stays_empty() {
        let mut buf = VecDeque::new();
        push_capped(&mut buf, 1, 0);
        assert!(buf.is_empty());
    }

    #[test]
    fn push_capped_f64_at_cap() {
        let mut buf = VecDeque::from(vec![1.0, 2.0, 3.0]);
        push_capped_f64(&mut buf, 4.0, 3);
        assert_eq!(buf.len(), 3);
        assert!((buf[0] - 2.0).abs() < f64::EPSILON);
        assert!((buf[2] - 4.0).abs() < f64::EPSILON);
    }
}
