set DATABASE_URL=sqlite://database.db?mode=rwc

cargo run --bin migration -- fresh
cargo run --bin webserver