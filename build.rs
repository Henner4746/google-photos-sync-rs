use std::{env, fs, path::Path};

const ICON_SIZE: i32 = 32;

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let output = env::var("OUT_DIR").expect("Cargo did not provide OUT_DIR");
        let icon = Path::new(&output).join("google-photos-sync.ico");
        write_icon(&icon).expect("Windows icon could not be generated");
        winresource::WindowsResource::new()
            .set_icon(icon.to_str().expect("icon path is not UTF-8"))
            .compile()
            .expect("Windows metadata resource could not be compiled");
    }
}

fn icon_pixel(x: i32, y: i32) -> u32 {
    let dx = x - ICON_SIZE / 2;
    let dy = y - ICON_SIZE / 2;
    let distance = dx * dx + dy * dy;
    let ring = (76..=126).contains(&distance);
    let forward = (20..=27).contains(&x) && (6..=13).contains(&y) && x + y >= 31;
    let back = (5..=12).contains(&x) && (19..=26).contains(&y) && x + y <= 31;
    if ring || forward || back {
        0xff_f2_f2_f2
    } else if distance <= 225 {
        0xff_11_11_11
    } else {
        0
    }
}

fn write_icon(path: &Path) -> std::io::Result<()> {
    let pixel_bytes = (ICON_SIZE * ICON_SIZE * 4) as u32;
    let mask_bytes = (ICON_SIZE * 4) as u32;
    let image_bytes = 40 + pixel_bytes + mask_bytes;
    let mut icon = Vec::with_capacity((22 + image_bytes) as usize);

    icon.extend_from_slice(&0_u16.to_le_bytes());
    icon.extend_from_slice(&1_u16.to_le_bytes());
    icon.extend_from_slice(&1_u16.to_le_bytes());
    icon.extend_from_slice(&[ICON_SIZE as u8, ICON_SIZE as u8, 0, 0]);
    icon.extend_from_slice(&1_u16.to_le_bytes());
    icon.extend_from_slice(&32_u16.to_le_bytes());
    icon.extend_from_slice(&image_bytes.to_le_bytes());
    icon.extend_from_slice(&22_u32.to_le_bytes());

    icon.extend_from_slice(&40_u32.to_le_bytes());
    icon.extend_from_slice(&ICON_SIZE.to_le_bytes());
    icon.extend_from_slice(&(ICON_SIZE * 2).to_le_bytes());
    icon.extend_from_slice(&1_u16.to_le_bytes());
    icon.extend_from_slice(&32_u16.to_le_bytes());
    icon.extend_from_slice(&0_u32.to_le_bytes());
    icon.extend_from_slice(&pixel_bytes.to_le_bytes());
    icon.extend_from_slice(&0_i32.to_le_bytes());
    icon.extend_from_slice(&0_i32.to_le_bytes());
    icon.extend_from_slice(&0_u32.to_le_bytes());
    icon.extend_from_slice(&0_u32.to_le_bytes());

    for y in (0..ICON_SIZE).rev() {
        for x in 0..ICON_SIZE {
            icon.extend_from_slice(&icon_pixel(x, y).to_le_bytes());
        }
    }
    for y in (0..ICON_SIZE).rev() {
        let mut row = 0_u32;
        for x in 0..ICON_SIZE {
            if icon_pixel(x, y) == 0 {
                row |= 1 << (31 - x);
            }
        }
        icon.extend_from_slice(&row.to_be_bytes());
    }

    fs::write(path, icon)
}
