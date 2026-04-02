//! `zestful test-focus` — cycle through all detected terminal URIs with focus.

use anyhow::Result;

pub fn run() -> Result<()> {
    let output = crate::workspace::inspect_terminals()?;

    let uris: Vec<String> = output
        .iter()
        .flat_map(|term| term.windows.iter())
        .flat_map(|win| win.tabs.iter())
        .filter_map(|tab| tab.uri.clone())
        .collect();

    if uris.is_empty() {
        eprintln!("zestful: no terminal tabs detected");
        return Ok(());
    }

    println!("Found {} terminal tab(s). Cycling through...", uris.len());

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        for uri in &uris {
            println!("  focusing: {}", uri);

            let parsed = crate::workspace::uri::parse_terminal_uri(uri);
            if let Some(p) = parsed {
                if let Err(e) = crate::workspace::terminals::handle_focus(
                    &p.app,
                    p.window_id.as_deref(),
                    p.tab_id.as_deref(),
                )
                .await
                {
                    eprintln!("    error: {}", e);
                }
            } else {
                eprintln!("    skipping invalid URI: {}", uri);
            }

            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });

    println!("Done.");
    Ok(())
}
