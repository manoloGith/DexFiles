//! DexFiles in a desktop window, to iterate without a device:
//! `cargo run -p dexfiles --features desktop -- [folder…]`.

fn main() {
    let launch = dexfiles::Launch {
        open: std::env::args().skip(1).map(Into::into).collect(),
        persistence_path: None,
    };
    let options_launch = launch.clone();
    dexfiles::run(
        move |renderer| dexfiles::native_options(renderer, &options_launch),
        launch,
    );
}
