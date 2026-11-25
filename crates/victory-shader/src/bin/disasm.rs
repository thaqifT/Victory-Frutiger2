//! Disassembles the GFX10 code of a console shader container: opcode names
//! and raw dwords, by offset. For reading a refusal the translator reports
//! ("GFX10 code at 0x13c8: ...").
//!
//! `cargo run -p victory-shader --bin disasm -- <container> [from] [to]`
//!
//! `from` and `to` are hex byte offsets (default: the whole program).

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(file) = args.first() else {
        eprintln!("usage: disasm <container.cdxil> [from] [to]");
        return ExitCode::from(2);
    };
    let bytes = match std::fs::read(file) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{file}: cannot read: {e}");
            return ExitCode::FAILURE;
        }
    };
    let hex = |s: Option<&String>| -> Option<usize> {
        let s = s?.trim();
        let s = s.strip_prefix("0x").unwrap_or(s);
        usize::from_str_radix(s, 16).ok()
    };
    let (from, to) = (hex(args.get(1)), hex(args.get(2)));
    let c = match victory_shader::Container::parse(&bytes) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{file}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let Some(part) = c.part(victory_shader::FourCC::DXIL) else {
        eprintln!("{file}: no DXIL part");
        return ExitCode::FAILURE;
    };
    let programs = match victory_shader::native_programs(part.data) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{file}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let Some(p) = programs
        .iter()
        .find(|p| matches!(p.stage, victory_shader::HwStage::Cs))
        .or_else(|| programs.first())
    else {
        eprintln!("{file}: no program");
        return ExitCode::FAILURE;
    };
    println!(
        "stage {:?}, wave{}, {} VGPRs, {} bytes of code",
        p.stage,
        if p.wave64 { 64 } else { 32 },
        p.vgprs,
        p.code.len()
    );
    let insts = match victory_shader::gfx10::decode(&p.code) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("decode: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("{} instructions", insts.len());
    for inst in &insts {
        if from.is_some_and(|f| inst.offset < f) || to.is_some_and(|t| inst.offset > t) {
            continue;
        }
        let words: Vec<String> = inst.words.iter().map(|w| format!("{w:08x}")).collect();
        println!(
            "{:6x}  {:<24} {:<24} {:?}",
            inst.offset,
            inst.name().unwrap_or("?"),
            words.join(" "),
            inst.fields(p.wave64)
        );
    }
    ExitCode::SUCCESS
}
