use rbatis::rbatis::Rbatis;
use rocket::{
    fairing::AdHoc,
    http::Status,
    request::{FromRequest, Outcome, Request},
    serde::{json::Json, Deserialize},
    State,
};
use serde_json::json;

mod util;
use util::{
    ApiKey, ApiKeyError, AuthRequest, PaymentCreate, SysResponse, TaskCreate,
    WebResponse,
};
use util::{PaymentReceive};

mod models;
use models::{Database, Payment, Task};

mod handlers;

#[macro_use]
extern crate rocket;

#[derive(Deserialize)]
pub struct Config {
    pub jwt_secret: String,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for ApiKey<'r> {
    type Error = ApiKeyError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        // Retrieve the config state like this
        let config = req.rocket().state::<Config>().unwrap();

        fn is_valid(key: &str, secret: &str) -> bool {
            matches!(util::decode_jwt(key, secret), Ok(_))
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
    "System online"
}

#[post("/auth/<pubkey>")]
async fn request_nonce(pubkey: &str, state: &State<Database>) -> WebResponse {
    handlers::authkey_request(pubkey, state).await
}

#[post("/auth", data = "<auth_request>")]
async fn post_nonce(
    auth_request: Json<AuthRequest<'_>>,
    config: &State<Config>,
    db: &State<Database>,
) -> WebResponse {
    handlers::authkey_parse(auth_request, config, db).await
}

#[post("/tasks", data = "<task_request>")]
async fn new_task(
    task_request: Json<TaskCreate<'_>>,
    database: &State<Database>,
    _auth: ApiKey<'_>,
) -> WebResponse {
    handlers::create_task(task_request, database).await
}

#[post("/payments", data = "<payment_request>")]
async fn new_payment(
    payment_request: Json<PaymentCreate<'_>>,
    database: &State<Database>,
    _auth: ApiKey<'_>,
) -> WebResponse {
    handlers::create_payment(payment_request, database).await
}

#[get("/tasks/<task_id>")]
async fn get_task(task_id: &str, database: &State<Database>, _auth: ApiKey<'_>) -> WebResponse {
    let db = &database.db;
    let fetch = Task::fetch_one_by_id(task_id, db).await;

    let query = match fetch {
        Ok(task) => task,
        Err(_) => {
            let data = json!({ "error": "Failed to query tasks" });
            let response = SysResponse { data };

            return (Status::BadRequest, Json(response));
        }
    };

    let task = match query {
        Some(task) => task,
        None => {
            let data = json!({ "error": "Not found" });
            let response = SysResponse { data };

            return (Status::NotFound, Json(response));
        }
    };

    let data = json!({ "task": task });
    let response = SysResponse { data };

    (Status::BadRequest, Json(response))
}

#[get("/tasks/<account>")]
async fn list_tasks(account: &str, database: &State<Database>, _auth: ApiKey<'_>) -> WebResponse {
    let db = &database.db;
    let fetch = Task::fetch_by_account(account, db).await;

    let tasks = match fetch {
        Ok(tasks) => tasks,
        Err(_) => {
            let data = json!({ "error": "Failed to fetch tasks" });
            let response = SysResponse { data };

            return (Status::BadRequest, Json(response));
        }
    };
    let data = json!({ "tasks": tasks });
    let response = SysResponse { data };

    (Status::BadRequest, Json(response))
}

#[get("/payments/<payment_id>")]
async fn get_payment(payment_id: &str, database: &State<Database>, _auth: ApiKey<'_>) -> WebResponse {
    let db = &database.db;
    let fetch = Payment::fetch_one_by_id(payment_id, db).await;

    let query = match fetch {
        Ok(query) => query,
        Err(_) => {
            let data = json!({ "error": "Failed to query payment" });
            let response = SysResponse { data };

            return (Status::BadRequest, Json(response));
        }
    };

    let payment = match query {
        Some(payment) => payment,
        None => {
            let data = json!({ "error": "Payment does not exist" });
            let response = SysResponse { data };

            return (Status::BadRequest, Json(response));
        }
    };

    let data = json!({ "payment": payment });
    let response = SysResponse { data };

    (Status::BadRequest, Json(response))
}

#[post("/payments/hook", data = "<payment_receive>")]
async fn receive_payment(
    payment_receive: Json<PaymentReceive<'_>>,
    database: &State<Database>,
    _auth: ApiKey<'_>,
) -> WebResponse {
    handlers::receive_payment(payment_receive, database).await
}

#[launch]
async fn rocket() -> _ {
    let server = rocket::build();
    let figment = server.figment();

    let _config: Config = figment.extract().expect("Config file not present");
    let _f = std::fs::File::create("database.db").unwrap();
    let rb = Rbatis::new();

    Database::migrate(&rb).await;

    server
        .mount("/", routes![index, request_nonce, post_nonce, new_task, new_payment, get_task, list_tasks, get_payment, receive_payment])
        .attach(AdHoc::config::<Config>())
        .manage(Database { db: rb })
}
