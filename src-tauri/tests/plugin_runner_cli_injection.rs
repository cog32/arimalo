//! Verifies that run_plugin injects the arimalo-query binary path and
//! prepends it to PATH so plugins can invoke CLI commands directly.

use std::fs;
use std::path::PathBuf;

use arimalo_covid::plugins;

fn tmp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "arimalo-plugin-inject-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn injects_arimalo_query_bin_and_path() {
    let root = tmp();

    // Fake bin dir containing a fake arimalo-query script
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let fake_query = bin_dir.join("arimalo-query");
    fs::write(&fake_query, "#!/bin/sh\necho fake\n").unwrap();
    let mut perms = fs::metadata(&fake_query).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o755);
    }
    fs::set_permissions(&fake_query, perms).unwrap();

    // Minimal plugin that echoes the context + query bin env
    let plugin_dir = root.join("plugins").join("probe");
    fs::create_dir_all(&plugin_dir).unwrap();
    fs::write(
        plugin_dir.join("plugin.toml"),
        r#"[plugin]
name = "probe"
version = "0.0.1"
script = "probe.py"
"#,
    )
    .unwrap();
    fs::write(
        plugin_dir.join("probe.py"),
        r#"import json, os, sys, subprocess
ctx = json.load(sys.stdin)
# Report what we got
print(json.dumps({
    "bin_ctx": ctx.get("bin", {}).get("arimalo_query"),
    "env": os.environ.get("ARIMALO_QUERY_BIN"),
    # arimalo-query on PATH resolves to something runnable
    "on_path": subprocess.run(["arimalo-query"], capture_output=True, text=True).stdout.strip(),
}))
"#,
    )
    .unwrap();

    let sources_dir = root.join("sources");
    fs::create_dir_all(&sources_dir).unwrap();

    // Override bin resolution
    std::env::set_var("ARIMALO_BIN_DIR", &bin_dir);
    let result = plugins::run_plugin(
        &plugin_dir,
        &sources_dir,
        &serde_json::Value::Object(Default::default()),
        &serde_json::Value::Object(Default::default()),
    );
    std::env::remove_var("ARIMALO_BIN_DIR");

    assert!(
        result.success,
        "plugin failed: stdout={} stderr={}",
        result.stdout, result.stderr
    );

    let parsed: serde_json::Value =
        serde_json::from_str(result.stdout.trim()).unwrap_or_else(|e| {
            panic!("bad plugin stdout JSON: {e}\n---\n{}", result.stdout);
        });

    let expected_bin = fake_query.to_string_lossy().to_string();
    assert_eq!(
        parsed["bin_ctx"].as_str().unwrap_or(""),
        expected_bin,
        "ctx.bin.arimalo_query should be the fake bin path"
    );
    assert_eq!(
        parsed["env"].as_str().unwrap_or(""),
        expected_bin,
        "ARIMALO_QUERY_BIN env should be the fake bin path"
    );
    assert_eq!(
        parsed["on_path"].as_str().unwrap_or(""),
        "fake",
        "bare `arimalo-query` on PATH should resolve to the fake bin"
    );
}
