use std::str::FromStr;

use chrono::{Utc};
use handlers::metadata::handle_update;
use migration::MigratorTrait;
use rocket::{
    fairing::{AdHoc, self},
    http::Status,
    request::{FromRequest, Outcome, Request},
    serde::{json::Json, Deserialize},
    State, Rocket, Build,
};
use serde_json::json;


mod util;
use util::{
    ApiKey, ApiKeyError, AuthRequest, PaymentCreate, SysResponse, TaskCreate,
    WebResponse, crypto, create_jwt,
};
use util::{PaymentReceive};

pub use entity::prelude::*;
use entity::accounts::Entity as Accounts;
use entity::payments::Entity as Payments;
use entity::tasks::Entity as Tasks;

use sea_orm::{entity::*, query::*};
use sea_orm_rocket::{Connection, Database};
use sea_orm::ActiveValue::NotSet;
use sea_orm::entity::prelude::Uuid;


use rocket::http::Header;
use rocket::{Response};
use rocket::fairing::{Fairing, Info, Kind};

pub struct CORS;

#[rocket::async_trait]
impl Fairing for CORS {
    fn info(&self) -> Info {
        Info {
            name: "Add CORS headers to responses",
            kind: Kind::Response
        }
    }

    async fn on_response<'r>(&self, _request: &'r Request<'_>, response: &mut Response<'r>) {
        response.set_header(Header::new("Access-Control-Allow-Origin", "*"));
        response.set_header(Header::new("Access-Control-Allow-Methods", "POST, GET, PATCH, OPTIONS"));
        response.set_header(Header::new("Access-Control-Allow-Headers", "*"));
        response.set_header(Header::new("Access-Control-Allow-Credentials", "true"));
    }
}

mod models;
mod handlers;

mod pool;
use pool::Db;

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
async fn request_nonce(pubkey: &str, connection: Connection<'_, Db>) -> WebResponse {
    let db = connection.into_inner();

    let fetch_account = Accounts::find()
        .filter(entity::accounts::Column::Pubkey.contains(pubkey))
        .one(db)
        .await;

    let account_query = match fetch_account {
        Ok(result) => result,
        Err(_) => {
            let data = json!({ "error": "Failed to save pubkey into database" });
            let response = SysResponse { data };

            return (Status::Accepted, Json(response));
        }
    };

    let account = if let Some(account) = account_query {
        let found_account: entity::accounts::ActiveModel = account.into();

        found_account
    } else {
        let nonce = Uuid::new_v4().to_string();
        let account = entity::accounts::ActiveModel {
            id: NotSet,
            pubkey: Set(pubkey.to_string()),
            nonce: Set(nonce.clone()),
            created_at: Set(Utc::now().naive_utc())
        };

        let account_copy = account.clone();

        let save_account = account.save(db).await;

        match save_account {
            Ok(_) => (),
            Err(_e) => {
                let data = json!({ "error": "Failed to save pubkey into database" });
                let response = SysResponse { data };

                return (Status::Accepted, Json(response));
            }
        };

        account_copy
    };
    
    let data = json!({ "nonce": account.nonce.as_ref() });
    let response = SysResponse { data };

    (Status::Accepted, Json(response))
}

#[post("/auth", data = "<auth_request>")]
async fn post_nonce(
    auth_request: Json<AuthRequest<'_>>,
    config: &State<Config>,
    connection: Connection<'_, Db>,
) -> WebResponse {
    let req = auth_request.into_inner();
    let db = connection.into_inner();

    // -- Fetch address-appropriate message from database
    let fetch_accounts = Accounts::find()
        .filter(entity::accounts::Column::Pubkey.contains(req.pubkey))
        .one(db)
        .await;

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
            if let Ok(token) = create_jwt(req.pubkey, &config.jwt_secret) {
                let data = json!({ "token": token });
                let response = SysResponse { data };

                (Status::Accepted, Json(response))
            } else {
                let data = json!({ "error": "Error creating auth token" });
                let response = SysResponse { data };

                (Status::InternalServerError, Json(response))
            }
        }
        Err(e) => {
            let data = json!({ "error": e.to_string() });
            let response = SysResponse { data };

            (Status::Forbidden, Json(response))
        }
    }
}

#[options("/tasks")]
async fn options_task() {}

#[post("/tasks", data = "<task_request>")]
async fn new_task(
    task_request: Json<TaskCreate<'_>>,
    connection: Connection<'_, Db>,
    _auth: ApiKey<'_>,
) -> WebResponse {
    // <- Receive Task Creation Request
    let request = task_request.into_inner();
    let db = connection.into_inner();

    // -- Calculate price
    let price: i32 = match crate::handlers::payment::check_price(request.mint_address).await {
        Ok(price) => price,
        Err(_) => {
            let data = json!({ "error": "Failed to calculate price" });
            let response = SysResponse { data };

            return (Status::InternalServerError, Json(response));
        }
    };

    // -- Query tasks for existing successful rankups
    let fetch_task = Tasks::find()
        .filter(entity::tasks::Column::MintAddress.contains(request.mint_address))
        .filter(entity::tasks::Column::Success.contains("true"))
        .one(db)
        .await;

    let query_task = match fetch_task {
        Ok(task) => task,
        Err(_) => {
            let data = json!({ "error": "Failed to fetch tasks for cooldown" });
            let response = SysResponse { data };

            return (Status::InternalServerError, Json(response));
        }
    };

    let task = entity::tasks::ActiveModel {
        id: NotSet,
        account: Set(request.account.to_string()),
        mint_address: Set(request.mint_address.to_string()),
        success: Set(false),
        created_at: Set(Utc::now().naive_utc()),
        price: Set(price)
    };

    let task_check = task.clone();

    // -- Check existing successful rankups if past cooldown period
    let _found_task = if let Some(existing_task) = query_task {
        let cooldown = 12;
        let time_difference = task.created_at.as_ref().time() - existing_task.created_at.time();
            if time_difference.num_hours() < cooldown {
                let data = json!({ "error": "NFT is in rankup cooldown" });
                let response = SysResponse { data };

                return (Status::BadRequest, Json(response));
            }
    };

    // -- Save Task
    match task.save(db).await {
        Ok(_) => (),
        Err(_e) => {
            let data = json!({ "error": "Failed to save task to database" });
            let response = SysResponse { data };

            return (Status::InternalServerError, Json(response));
        }
    }

    // -> Send Task Created Response
    let data = json!({ "task_id": task_check.id.as_ref() });
    let response = SysResponse { data };

    (Status::Created, Json(response))
}

#[options("/payments")]
async fn options_payments() {}

#[post("/payments", data = "<payment_request>")]
async fn new_payment(
    payment_request: Json<PaymentCreate<'_>>,
    connection: Connection<'_, Db>,
    _auth: ApiKey<'_>,
) -> WebResponse {
    let request = payment_request.into_inner();
    let db = connection.into_inner();

    let query = Tasks::find_by_id(request.task_id)
        .one(db)
        .await;
    
    let task = {
        let fetch_task = match query {
            Ok(fetch) => fetch,
            Err(_) => {
                let data = json!({ "error": "Database query failed" });
                let response = SysResponse { data };

                return (Status::InternalServerError, Json(response));
            }
        };

        match fetch_task {
            Some(task) => task,
            None => {
                let data = json!({ "error": "Task does not exist" });
                let response = SysResponse { data };

                return (Status::NotFound, Json(response));
            }
        }
    };

    let new_payment = entity::payments::ActiveModel {
        id: NotSet,
        account: Set(request.account.to_string()),
        success: Set(false),
        created_at: Set(Utc::now().naive_utc()),
        tx: Set(String::from("")),
        task_id: Set(request.task_id),
        amount: Set(task.price)
    };

    let payment_clone = new_payment.clone();

    match new_payment.save(db).await {
        Ok(_) => (),
        Err(_) => {
            let data = json!({ "error": "Failed to save payment to database" });
            let response = SysResponse { data };

            return (Status::InternalServerError, Json(response));
        }
    }

    // -> paymentid
    let data = json!({ "payment_id": payment_clone.id.as_ref() });
    let response = SysResponse { data };

    (Status::Created, Json(response))
}

#[options("/tasks/id/<task_id>")]
async fn options_task_2(task_id: &str) {}

#[get("/tasks/id/<task_id>")]
async fn get_task(task_id: &str, connection: Connection<'_, Db>, _auth: ApiKey<'_>) -> WebResponse {
    let db = connection.into_inner();
    let fetch = Tasks::find()
        .filter(entity::tasks::Column::Id.contains(task_id))
        .one(db)
        .await;

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

#[options("/tasks/accounts/<account>")]
async fn options_task_3(account: &str) {}

#[get("/tasks/account/<account>")]
async fn list_tasks(account: &str, connection: Connection<'_, Db>, _auth: ApiKey<'_>) -> WebResponse {
    let db = connection.into_inner();
    let fetch = Tasks::find()
        .filter(entity::tasks::Column::Account.contains(account))
        .all(db)
        .await;

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

#[options("/payments/id/<payment>")]
async fn options_payments_2(payment: &str) {}

#[get("/payments/id/<payment_id>")]
async fn get_payment(payment_id: &str, connection: Connection<'_, Db>, _auth: ApiKey<'_>) -> WebResponse {
    let db = connection.into_inner();
    let fetch = Payments::find()
        .filter(entity::payments::Column::Id.contains(payment_id))
        .one(db)
        .await;

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
    connection: Connection<'_, Db>,
    _auth: ApiKey<'_>,
) -> WebResponse {
    let request = payment_receive.into_inner();
    let db = connection.into_inner();

    let fetch_payment_by_id = Payments::find()
        .filter(entity::payments::Column::Id.contains(request.payment_id))
        .one(db)
        .await;

    let mut payment = {
        let fetch_payment = match fetch_payment_by_id {
            Ok(found) => found,
            Err(_) => {
                let data = json!({ "error": "Failed to fetch payment" });
                let response = SysResponse { data };

                return (Status::InternalServerError, Json(response));
            }
        };

        match fetch_payment {
            Some(payment) => payment,
            None => {
                let data = json!({ "error": "Payment does not exist" });
                let response = SysResponse { data };

                return (Status::NotFound, Json(response));
            }
        }
    };

    // Verify Transaction Signature
    let signature = match solana_sdk::signature::Signature::from_str(request.tx_id) {
        Ok(signature) => signature,
        Err(_) => {
            let data = json!({ "error": "Invalid signature" });
            let response = SysResponse { data };
            
            return (Status::BadRequest, Json(response));
        }
    };

    let _confirm_result = match crate::handlers::payment::confirm_transaction(&signature).await {
        Ok(_) => (),
        Err(_) => {
            let data = json!({ "error": "Invalid signature" });
            let response = SysResponse { data };
            
            return (Status::BadRequest, Json(response));
        }
    };
    payment.success = true;
    let payment_clone = payment.clone();
    let updated_payment: entity::payments::ActiveModel = payment.into();

    // Set Payment to Success
    let _confirm_payment = match updated_payment.save(db).await {
        Ok(_) => (),
        Err(_) => {
            let data = json!({ "error": "Failed to update payment" });
            let response = SysResponse { data };
            
            return (Status::BadRequest, Json(response));
        }
    };

    let fetch_task_by_id = Tasks::find_by_id(payment_clone.id)
        .one(db)
        .await;

    let mut task = {
        let fetch_task = match fetch_task_by_id {
            Ok(fetch) => fetch,
            Err(_) => {
                let data = json!({ "error": "Database query failed" });
                let response = SysResponse { data };

                return (Status::InternalServerError, Json(response));
            }
        };

        match fetch_task {
            Some(task) => task,
            None => {
                let data = json!({ "error": "Task does not exist" });
                let response = SysResponse { data };

                return (Status::NotFound, Json(response));
            }
        }
    };

    let _update_metadata = match handle_update(&task.mint_address).await {
        Ok(_) => {},
        Err(e) => return e
    };

    task.success = true;
    let updated_task: entity::tasks::ActiveModel = task.into();
    let _update_task = match updated_task.save(db).await {
        Ok(_) => (),
        Err(_) => {
            let data = json!({ "error": "Failed to update task" });
            let response = SysResponse { data };

            return (Status::NotFound, Json(response));
        }
    };

    let data = json!({ "error": "Task does not exist" });
    let response = SysResponse { data };

    (Status::NotFound, Json(response))
}

async fn run_migrations(rocket: Rocket<Build>) -> fairing::Result {
    let conn = &Db::fetch(&rocket).unwrap().conn;
    let _ = migration::Migrator::up(conn, None).await;
    Ok(rocket)
}

#[launch]
async fn rocket() -> _ {
    let server = rocket::build();
    let figment = server.figment();

    let _config: Config = figment.extract().expect("Config file not present");

    server
        .attach(Db::init())
        .attach(AdHoc::try_on_ignite("Migrations", run_migrations))
        .attach(AdHoc::config::<Config>())
        .mount("/api", routes![index, request_nonce, post_nonce, new_task, new_payment, get_task, list_tasks, get_payment, receive_payment, options_task, options_task_2, options_task_3, options_payments, options_payments_2])
        .attach(CORS)
}
