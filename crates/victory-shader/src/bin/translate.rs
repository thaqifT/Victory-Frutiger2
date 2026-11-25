//! Translates shader containers offline, the way a run does, so a shader
//! that does not translate can be replayed without the game.
//!
//! `cargo run -p victory-shader --bin translate -- <container>...`
//!
//! Prints the stage, the error, and (with `VICTORY_SHADER_SAVE=<dir>`) the
//! translated DXIL of the ones that do translate. Exit code is 1 if any
//! container failed.

use std::process::ExitCode;

fn main() -> ExitCode {
    let files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() {
        eprintln!("usage: translate <container.cdxil>...");
        return ExitCode::from(2);
    }
    let out = std::env::var_os("VICTORY_SHADER_SAVE").map(std::path::PathBuf::from);
    let mut failed = 0u32;
    for file in &files {
        let bytes = match std::fs::read(file) {
            Ok(b) => b,
            Err(e) => {
                println!("{file}: cannot read: {e}");
                failed += 1;
                continue;
            }
        };
        let stage = victory_shader::Container::parse(&bytes)
            .ok()
            .and_then(|c| {
                let part = c.part(victory_shader::FourCC::DXIL)?;
                victory_shader::native_programs(part.data)
                    .ok()?
                    .into_iter()
                    .find(|p| {
                        matches!(
                            p.stage,
                            victory_shader::HwStage::Vs
                                | victory_shader::HwStage::Ps
                                | victory_shader::HwStage::Cs
                                | victory_shader::HwStage::Ms
                        )
                    })
                    .map(|p| p.stage)
            });
        let stem = std::path::Path::new(file)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "shader".to_owned());
        match victory_shader::emit::translate(&bytes) {
            Ok(host) => {
                println!(
                    "{stem}: ok, stage {:?}, {} bytes of DXIL, {} user-data dwords, {} samplers",
                    host.stage,
                    host.dxil.len(),
                    host.user_data.len(),
                    host.samplers.len()
                );
                if let Some(dir) = &out {
                    let _ = std::fs::create_dir_all(dir);
                    let _ = std::fs::write(dir.join(format!("{stem}.dxil")), &host.dxil);
                }
            }
            Err(e) => {
                println!("{stem}: {e}");
                if let Some(stage) = stage {
                    println!("  stage {stage:?}");
                }
                failed += 1;
            }
        }
    }
    if failed > 0 {
        println!("{failed} of {} did not translate", files.len());
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
