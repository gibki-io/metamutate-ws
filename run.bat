set DATABASE_URL=sqlite://database.db?mode=rwc

cargo run --bin migration
cargo run --bin webserver