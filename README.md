# Gibki Metamutate
## Backend Service for Metadata Mutation Web Service

<br />

### Get Started
0. Install latest Rust
1. Rename _RocketExample.toml_
2. Change field _jwt_secret_
```
# Project Config
jwt_secret = "<CHANGE ME>"
```
3. Run project for development and testing
```
cargo run --bin webserver
```
4. Build project for release
```
cargo build --release
```

<br />

### Todo
1. Code refactor to handlers, routes, etc.
2. Error handling
3. Fix modularity
4. Testing