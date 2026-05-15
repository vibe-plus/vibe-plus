fn main() {
    // Ensure the web dist directory exists so rust-embed can find it at compile time.
    // In CI the web app is built first; locally developers use `--dev` mode and don't
    // need embedded assets.
    let dist = std::path::Path::new("../web/dist");
    if !dist.exists() {
        std::fs::create_dir_all(dist).expect("failed to create apps/web/dist placeholder");
        std::fs::write(
            dist.join("index.html"),
            b"<!DOCTYPE html><html><body>\
              <p>UI not built. Run: <code>cd apps/web &amp;&amp; bun run build</code></p>\
              </body></html>",
        )
        .expect("failed to write placeholder index.html");
    }
    println!("cargo:rerun-if-changed=../web/dist");
    println!("cargo:rerun-if-changed=build.rs");
}
