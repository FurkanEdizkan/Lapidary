use std::process::Command;

const PNG_MAGIC: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
const BG_R: u8 = 13;
const BG_G: u8 = 13;
const BG_B: u8 = 14;

fn cube_stl() -> std::path::PathBuf {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("../fixtures/cube.stl")
}

#[test]
fn render_thumb_64px() {
    let bin = env!("CARGO_BIN_EXE_rust-mesh");
    let cube = cube_stl();
    assert!(cube.exists(), "fixture cube.stl not found at {:?}", cube);

    let thumb_path = std::env::temp_dir().join(format!("lapidary_test_thumb_{}.png", std::process::id()));
    // Clean up any leftover from a previous run
    let _ = std::fs::remove_file(&thumb_path);

    let output = Command::new(bin)
        .arg(cube.to_str().unwrap())
        .arg("--thumb")
        .arg(thumb_path.to_str().unwrap())
        .arg("--size")
        .arg("64")
        .arg("--json")
        .output()
        .expect("failed to spawn rust-mesh");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "rust-mesh exited with status {:?}\nstdout: {}\nstderr: {}",
        output.status,
        stdout,
        stderr
    );

    // 1. PNG file must exist
    assert!(
        thumb_path.exists(),
        "thumb PNG was not written to {:?}",
        thumb_path
    );

    let png_bytes = std::fs::read(&thumb_path).expect("could not read thumb PNG");

    // 2. Must start with PNG magic bytes
    assert_eq!(
        &png_bytes[..8],
        &PNG_MAGIC,
        "file does not start with PNG magic"
    );

    // 3. Decode with the png crate — must be 64×64
    let decoder = png::Decoder::new(std::io::Cursor::new(&png_bytes));
    let mut reader = decoder.read_info().expect("png decode failed");
    let mut img_buf = vec![0u8; reader.output_buffer_size()];
    let frame_info = reader.next_frame(&mut img_buf).expect("png read_frame failed");
    assert_eq!(frame_info.width, 64, "expected width 64, got {}", frame_info.width);
    assert_eq!(frame_info.height, 64, "expected height 64, got {}", frame_info.height);

    // 4. NOT all pixels == background #0d0d0e (cube must be drawn)
    let bytes = &img_buf[..frame_info.buffer_size()];
    // RGBA layout: 4 bytes per pixel
    let stride = 4usize;
    let all_bg = bytes.chunks(stride).all(|px| {
        px[0] == BG_R && px[1] == BG_G && px[2] == BG_B
    });
    assert!(
        !all_bg,
        "all pixels are background color — cube was not drawn"
    );

    // 5. stdout must contain the bbox value 20 (cube.stl is 20mm)
    assert!(
        stdout.contains("20"),
        "expected bbox '20' in stdout, got: {}",
        stdout
    );

    // Cleanup
    let _ = std::fs::remove_file(&thumb_path);
}
