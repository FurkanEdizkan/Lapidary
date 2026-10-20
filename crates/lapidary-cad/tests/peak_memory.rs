//! A32: how much memory reading and measuring one mesh takes at its peak, for a file named by `LAPIDARY_PEAK_FILE`.
//! Ignored, because it needs a real large file: `LAPIDARY_PEAK_FILE=<path> cargo test --release -p lapidary-cad
//! --test peak_memory -- --ignored --nocapture`. Linux only (`VmHWM`).

fn high_water_mib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
        })
        .map_or(0, |kib| kib / 1024)
}

#[test]
#[ignore = "needs LAPIDARY_PEAK_FILE, a real large mesh"]
fn peak_memory_of_one_file() {
    let path = std::env::var("LAPIDARY_PEAK_FILE").expect("LAPIDARY_PEAK_FILE names a mesh");
    let format = path
        .rsplit('.')
        .next()
        .expect("an extension")
        .to_ascii_lowercase();
    let bytes = std::fs::read(&path).expect("reads the file");
    let size = bytes.len() as u64 >> 20;
    let before = high_water_mib();
    let mesh = lapidary_cad::parse(&bytes, &format).expect("parses");
    let after_parse = high_water_mib();
    drop(bytes);
    let (m, _) = lapidary_cad::measure(&mesh);
    println!(
        "{size} MiB {format}: {} triangles; peak {before} MiB with the file read, {after_parse} after parse, {} after measure",
        m.triangle_count,
        high_water_mib()
    );
    println!(
        "  figures: watertight {}, volume {:?}, area {:.3}, bbox {:?}",
        m.is_watertight, m.volume_mm3, m.surface_area_mm2, m.bbox_mm
    );
}
