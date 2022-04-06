use rbatis::rbatis::Rbatis;
use rocket::{
    fairing::AdHoc,
    http::Status,
    request::{FromRequest, Outcome, Request},
    serde::{json::Json, Deserialize, Serialize},
    State,
};
use serde_json::json;
use uuid::Uuid;

mod util;
use util::crypto;
use util::{create_jwt, ApiKey, ApiKeyError, AuthRequest, Claims, SysResponse, WebResponse};

mod models;
use models::{Database, WalletAccount};

#[macro_use]
extern crate rocket;

#[derive(Deserialize)]
struct Config {
    jwt_secret: String
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for ApiKey<'r> {
    type Error = ApiKeyError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        // Retrieve the config state like this
        let config = req.rocket().state::<Config>().unwrap();

        fn is_valid(key: &str, secret: &str) -> bool {
            true
        }

        match req.headers().get_one("Authorization") {
            None => Outcome::Failure((Status::Unauthorized, ApiKeyError::Missing)),
            Some(key) if is_valid(key, &config.jwt_secret) => Outcome::Success(ApiKey(key)),
            Some(_) => Outcome::Failure((Status::Unauthorized, ApiKeyError::Invalid)),
        }
    }
}

#[get("/")]
async fn index() -> &'static str {
    "Hello, world!"
}

#[post("/auth/<pubkey>")]
async fn request_nonce(pubkey: &str, state: &State<Database>) -> WebResponse {
    let nonce = Uuid::new_v4();

    let account = WalletAccount {
        pubkey: pubkey.to_string(),
        nonce: nonce.to_string(),
        created_at: rbatis::DateUtc::now(),
    };

    account
        .save(&state.inner().db)
        .await
        .expect("Failed to save");

    let data = json!({ "nonce": nonce });

    let response = SysResponse { data };

    (Status::Accepted, Json(response))
}

#[post("/auth", data = "<auth_request>")]
async fn post_nonce(
    auth_request: Json<AuthRequest<'_>>,
    config: &State<Config>,
    db: &State<Database>,
) -> WebResponse {
    let req = auth_request.into_inner();
    // -- Fetch address-appropriate message from database
    let db = &db.db;
    let fetch_accounts = WalletAccount::lookup(&req.pubkey, db).await;
    let account = if let Ok(Some(account)) = fetch_accounts {
        account
    } else {
        let data = json!({ "error": "Pubkey has no auth key registered" });
        let response = SysResponse { data };

        return (Status::Forbidden, Json(response));
    };
    // <- Signature
    match crypto::verify_message(&req, &account.nonce) {
        Ok(_) => {
            // -> Api Key
            if let Ok(token) = create_jwt(&req.pubkey, &config.jwt_secret) {
                let data = json!({ "token": token });
                let response = SysResponse { data };

                (Status::Accepted, Json(response))
            } else {
                let data = json!({ "error": "Error creating auth token" });
                let response = SysResponse { data };

                (Status::InternalServerError, Json(response))
            }
        }
        Err(_e) => {
            let data = json!({ "error": "Signature does not match pubkey" });
            let response = SysResponse { data };

            (Status::Forbidden, Json(response))
        }
    }
}

#[post("/tasks", data = "<task_request>")]
async fn new_task(task_request: String, auth: ApiKey<'_>) {
    // <- {mintAddress, currentRank}
    // -> taskid
    todo!()
}

#[post("/payments")]
async fn new_payment(auth: ApiKey<'_>) {
    todo!()
}

#[get("/tasks/<taskid>")]
async fn get_task(taskid: &str, auth: ApiKey<'_>) {
    todo!()
}

#[get("/payments/<paymentid>")]
async fn get_payment(paymentid: &str, auth: ApiKey<'_>) {
    todo!()
}
#[launch]
async fn rocket() -> _ {
    let server = rocket::build();
    let figment = server.figment();

    let _config: Config = figment.extract().expect("Config file not present");
    let _rb = Rbatis::new();
    let _f = std::fs::File::create("database.db").unwrap();
    let rb = Rbatis::new();

    rb.link("sqlite://database.db")
        .await
        .expect("Failed to connect to DB");
    rb.exec("CREATE TABLE IF NOT EXISTS accounts(pubkey VARCHAR(60), nonce VARCHAR(60), created_at DATE, PRIMARY KEY(pubkey))", vec![])
        .await
        .expect("Failed to create table");

    server
        .mount("/", routes![index, request_nonce, post_nonce])
        .attach(AdHoc::config::<Config>())
        .manage(Database { db: rb })
}
