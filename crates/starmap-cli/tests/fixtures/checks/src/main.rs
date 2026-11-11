#[derive(starmap::Env)]
struct ShelfEnv {
    #[starmap("SHELF_PORT", check = unprivileged)]
    port: starmap::Port,
    #[starmap("SHELF_REGION")]
    region: String,
}
