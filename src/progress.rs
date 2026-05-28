pub fn step(message: impl AsRef<str>) {
    eprintln!("shelf > {}", message.as_ref());
}

pub fn ok(message: impl AsRef<str>) {
    eprintln!("shelf ✓ {}", message.as_ref());
}
