/// Short, sortable, prefixed identifiers such as `run_0192f3c4a1b2c3d4`.
pub fn new_id(prefix: &str) -> String {
    let u = uuid::Uuid::now_v7().simple().to_string();
    // first 12 hex chars = millisecond timestamp, then 8 random chars
    format!("{prefix}_{}{}", &u[..12], &u[24..32])
}

#[cfg(test)]
mod tests {
    #[test]
    fn ids_are_prefixed_and_unique() {
        let a = super::new_id("run");
        let b = super::new_id("run");
        assert!(a.starts_with("run_"));
        assert_ne!(a, b);
        assert_eq!(a.len(), 4 + 20);
    }
}
