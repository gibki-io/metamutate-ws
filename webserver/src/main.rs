use chrono::Utc;
use handlers::metadata::handle_update;
use migration::MigratorTrait;
use rocket::{
    fairing::{self, AdHoc},
    http::Status,
    request::{FromRequest, Outcome, Request},
    serde::{json::Json, Deserialize},
    Build, Rocket, State,
};
use serde_json::json;

mod util;
use util::PaymentReceive;
use util::{
    create_jwt, crypto, ApiKey, ApiKeyError, AuthRequest, PaymentCreate, SysResponse, TaskCreate,
    WebResponse,
};

use entity::accounts::Entity as Accounts;
use entity::history::Entity as History;
use entity::payments::Entity as Payments;
pub use entity::prelude::*;
use entity::tasks::Entity as Tasks;

use sea_orm::entity::prelude::Uuid;
use sea_orm::ActiveValue::NotSet;
use sea_orm::{entity::*, query::*};
use sea_orm_rocket::{Connection, Database};

use rocket::fairing::{Fairing, Info, Kind};
use rocket::http::Header;
use rocket::Response;

pub struct CORS;

#[rocket::async_trait]
impl Fairing for CORS {
    fn info(&self) -> Info {
        Info {
            name: "Add CORS headers to responses",
            kind: Kind::Response,
        }
    }

    async fn on_response<'r>(&self, _request: &'r Request<'_>, response: &mut Response<'r>) {
        response.set_header(Header::new("Access-Control-Allow-Origin", "*"));
        response.set_header(Header::new(
            "Access-Control-Allow-Methods",
            "POST, GET, PATCH, OPTIONS",
        ));
        response.set_header(Header::new("Access-Control-Allow-Headers", "*"));
        response.set_header(Header::new("Access-Control-Allow-Credentials", "true"));
    }
}

mod handlers;
mod models;

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
            created_at: Set(Utc::now().naive_utc()),
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
        Err(e) => {
            let data = json!({ "error": e.to_string() });
            let response = SysResponse { data };

            return (Status::InternalServerError, Json(response));
        }
    };

    // -- Query tasks for existing successful rankups
    let fetch_task = History::find()
        .filter(entity::history::Column::MintAddress.contains(request.mint_address))
        .filter(entity::history::Column::Success.eq(true))
        .order_by_desc(entity::history::Column::FinishedAt)
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
        price: Set(price),
    };

    // -- Check existing successful rankups if past cooldown period
    let _found_task = if let Some(history) = query_task {
        let cooldown = 12;
        let time_difference = task.created_at.as_ref().time() - history.finished_at.time();
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
    };

    let fetch_task = Tasks::find()
        .filter(entity::tasks::Column::MintAddress.contains(request.mint_address))
        .filter(entity::tasks::Column::Account.contains(request.account))
        .order_by_desc(entity::tasks::Column::CreatedAt)
        .one(db)
        .await;

    let task_query = match fetch_task {
        Ok(query) => query,
        Err(e) => {
            let data = json!({ "error": e.to_string() });
            let response = SysResponse { data };

            return (Status::InternalServerError, Json(response));
        }
    };

    let return_task = match task_query {
        Some(task) => task,
        None => {
            let data = json!({ "error": "Missing task" });
            let response = SysResponse { data };

            return (Status::NotFound, Json(response));
        }
    };

    // -> Send Task Created Response
    let data = json!({ "task_id": return_task.id });
    let response = SysResponse { data };

    (Status::Created, Json(response))
}

#[post("/payments", data = "<payment_request>")]
async fn new_payment(
    payment_request: Json<PaymentCreate<'_>>,
    connection: Connection<'_, Db>,
    _auth: ApiKey<'_>,
) -> WebResponse {
    let request = payment_request.into_inner();
    let db = connection.into_inner();

    let query = Tasks::find_by_id(request.task_id).one(db).await;

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
        tx: Set(String::from("none")),
        task_id: Set(request.task_id),
        amount: Set(task.price),
    };

    match new_payment.save(db).await {
        Ok(_) => (),
        Err(e) => {
            let data = json!({ "error": e.to_string() });
            let response = SysResponse { data };

            return (Status::InternalServerError, Json(response));
        }
    }

    let return_payment = Payments::find()
        .filter(entity::payments::Column::TaskId.eq(request.task_id))
        .order_by_desc(entity::payments::Column::CreatedAt)
        .one(db)
        .await;

    let payment_query = match return_payment {
        Ok(query) => query,
        Err(e) => {
            let data = json!({ "error": e.to_string() });
            let response = SysResponse { data };

            return (Status::NotFound, Json(response));
        }
    };

    let payment_clone = match payment_query {
        Some(payment) => payment,
        None => {
            let data = json!({ "error": "Missing payment object" });
            let response = SysResponse { data };

            return (Status::NotFound, Json(response));
        }
    };

    // -> paymentid
    let data = json!({ "payment_id": payment_clone.id });
    let response = SysResponse { data };

    (Status::Created, Json(response))
}

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

    (Status::Accepted, Json(response))
}

#[get("/tasks/account/<account>")]
async fn list_tasks(
    account: &str,
    connection: Connection<'_, Db>,
    _auth: ApiKey<'_>,
) -> WebResponse {
    let db = connection.into_inner();
    let fetch = Tasks::find()
        .filter(entity::tasks::Column::Account.contains(account))
        .order_by_desc(entity::tasks::Column::Id)
        .paginate(db, 10)
        .fetch()
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

    (Status::Accepted, Json(response))
}

#[get("/payments/id/<payment_id>")]
async fn get_payment(
    payment_id: &str,
    connection: Connection<'_, Db>,
    _auth: ApiKey<'_>,
) -> WebResponse {
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

#[post("/payments/account/<account>")]
async fn list_payments(
    account: &str,
    connection: Connection<'_, Db>,
    _auth: ApiKey<'_>,
) -> WebResponse {
    let db = connection.into_inner();

    let fetch_payments = Payments::find()
        .filter(entity::payments::Column::Account.contains(account))
        .order_by_desc(entity::payments::Column::Id)
        .paginate(db, 10)
        .fetch()
        .await;

    let payments = match fetch_payments {
        Ok(payments) => payments,
        Err(_) => {
            let data = json!({ "error": "Failed to fetch payments" });
            let response = SysResponse { data };

            return (Status::InternalServerError, Json(response));
        }
    };

    let data = json!({ "payments": payments });
    let response = SysResponse { data };

    (Status::Accepted, Json(response))
}

#[post("/history/account/<account>")]
async fn list_history(
    account: &str,
    connection: Connection<'_, Db>,
    _auth: ApiKey<'_>,
) -> WebResponse {
    let db = connection.into_inner();

    let fetch_history = History::find()
        .filter(entity::history::Column::Account.contains(account))
        .order_by_desc(entity::history::Column::Id)
        .paginate(db, 10)
        .fetch()
        .await;

    let history = match fetch_history {
        Ok(history) => history,
        Err(e) => {
            let data = json!({ "error": e.to_string() });
            let response = SysResponse { data };

            return (Status::InternalServerError, Json(response));
        }
    };

    let data = json!({ "history": history });
    let response = SysResponse { data };

    (Status::Accepted, Json(response))
}

#[post("/payments/hook", data = "<payment_receive>")]
async fn receive_payment(
    payment_receive: Json<PaymentReceive<'_>>,
    connection: Connection<'_, Db>,
    _auth: ApiKey<'_>,
) -> WebResponse {
    type PaymentsModel = entity::payments::Model;
    type TasksModel = entity::tasks::Model;

    let request = payment_receive.into_inner();
    let db = connection.into_inner();

    let fetch_payment_by_id: Result<Option<PaymentsModel>, sea_orm::DbErr> = Payments::find()
        .filter(entity::payments::Column::Id.eq(request.payment_id))
        .one(db)
        .await;

    let payment: PaymentsModel = {
        let fetch_payment: Option<PaymentsModel> = match fetch_payment_by_id {
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

    let fetch_task_by_id = Tasks::find_by_id(payment.task_id).one(db).await;

    let task: TasksModel = {
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

    let success = match handle_update(&task.mint_address).await {
        Ok(val) => val,
        Err(e) => {
            let data = json!({ "error": e.to_string() });
            let response = SysResponse { data };

            return (Status::InternalServerError, Json(response));
        }
    };

    let new_history = entity::history::ActiveModel {
        id: NotSet,
        account: Set(payment.account),
        mint_address: Set(task.mint_address),
        finished_at: Set(Utc::now().naive_utc()),
        payment_id: Set(payment.id),
        task_id: Set(task.id),
        signature: Set(request.tx_id.to_string()),
        price: Set(task.price),
        success: Set(success),
    };

    match new_history.save(db).await {
        Ok(_) => (),
        Err(e) => {
            let data = json!({ "error": e.to_string() });
            let response = SysResponse { data };

            return (Status::InternalServerError, Json(response))
        }
    }

    let data = json!({ "message": "Payment successful" });
    let response = SysResponse { data };

    (Status::Accepted, Json(response))
}

async fn run_migrations(rocket: Rocket<Build>) -> fairing::Result {
    let conn = &Db::fetch(&rocket).unwrap().conn;
    let _ = migration::Migrator::up(conn, None).await;
    Ok(rocket)
}

#[derive(Responder)]
struct OptionsResponder<'a> {
    status: Status,
    origin: Header<'a>,
    methods: Header<'a>,
    headers: Header<'a>,
    credentials: Header<'a>,
}

#[catch(default)]
async fn options_for_all() -> OptionsResponder<'static> {
    OptionsResponder {
        status: Status::Accepted,
        origin: Header::new("Access-Control-Allow-Origin", "*"),
        methods: Header::new("Access-Control-Allow-Methods", "POST, GET, PATCH, OPTIONS"),
        headers: Header::new("Access-Control-Allow-Headers", "*"),
        credentials: Header::new("Access-Control-Allow-Credentials", "true"),
    }
}

#[launch]
async fn rocket() -> _ {
    let server = rocket::build();
    let figment = server.figment();

    let _config: Config = figment.extract().expect("Config file not present");

    server
        .attach(CORS)
        .attach(Db::init())
        .attach(AdHoc::try_on_ignite("Migrations", run_migrations))
        .attach(AdHoc::config::<Config>())
        .register("/api", catchers![options_for_all])
        .mount(
            "/api",
            routes![
                index,
                request_nonce,
                post_nonce,
                new_task,
                new_payment,
                get_task,
                get_payment,
                list_tasks,
                list_payments,
                list_history,
                receive_payment
            ],
        )
}
