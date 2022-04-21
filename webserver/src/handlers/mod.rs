use std::error::Error;
use std::str::FromStr;

use rbatis::crud::CRUD;
use rbatis::rbatis::Rbatis;
use rocket::http::Status;
use rocket::serde::json::Json;
use rocket::State;
use serde_json::json;
use uuid::Uuid;

use crate::handlers::metadata::handle_update;
use crate::models::{Database, Payment, Task, WalletAccount};
use crate::util::{
    create_jwt, crypto, AuthRequest, PaymentCreate, PaymentReceive, SysResponse, TaskCreate,
    WebResponse,
};
use crate::Config;

pub mod metadata;
pub mod payment;

pub async fn authkey_request(pubkey: &str, state: &State<Database>) -> WebResponse {
    let nonce = Uuid::new_v4();

    let account = WalletAccount {
        pubkey: pubkey.to_string(),
        nonce: nonce.to_string(),
        created_at: rbatis::DateTimeUtc::now(),
    };

    let save_account = account.save(&state.inner().db).await;

    match save_account {
        Ok(_) => (),
        Err(_e) => {
            let data = json!({ "error": "Failed to save pubkey into database" });
            let response = SysResponse { data };

            return (Status::Accepted, Json(response));
        }
    }

    let data = json!({ "nonce": nonce });
    let response = SysResponse { data };

    (Status::Accepted, Json(response))
}

pub async fn authkey_parse(
    auth_request: Json<AuthRequest<'_>>,
    config: &State<Config>,
    db: &State<Database>,
) -> WebResponse {
    let req = auth_request.into_inner();

    // -- Fetch address-appropriate message from database
    let db: &Rbatis = &db.db;
    let fetch_accounts = WalletAccount::fetch(&req.pubkey, db).await;

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

pub async fn create_task(task_request: Json<TaskCreate<'_>>, db: &State<Database>) -> WebResponse {
    // <- Receive Task Creation Request
    let request = task_request.into_inner();
    let db = &db.db;

    // -- Calculate price
    let price: i64 = match crate::handlers::payment::check_price(request.mint_address, &db).await {
        Ok(price) => price,
        Err(_) => {
            let data = json!({ "error": "Failed to calculate price" });
            let response = SysResponse { data };

            return (Status::InternalServerError, Json(response));
        }
    };

    // -- Query tasks for existing successful rankups
    let check_cooldown = db.new_wrapper()
        .eq("mint_address", &request.mint_address)
        .eq("success", true)
        .order_by(true, &["date"]);
    let fetch_task: Result<Option<Task>, rbatis::Error> = db.fetch_by_wrapper(check_cooldown).await;
    let query_task = match fetch_task {
        Ok(task) => task,
        Err(_) => {
            let data = json!({ "error": "Failed to fetch tasks for cooldown" });
            let response = SysResponse { data };

            return (Status::InternalServerError, Json(response));
        }
    };

    let task = Task::new(request, price);

    // -- Check existing successful rankups if past cooldown period
    let _found_task = if let Some(existing_task) = query_task {
        let cooldown = 12;
        let time_difference = task.created_at.time() - existing_task.created_at.time();
            if time_difference.num_hours() < cooldown {
                let data = json!({ "error": "NFT is in rankup cooldown" });
                let response = SysResponse { data };

                return (Status::BadRequest, Json(response));
            }
        ()
    } else {
        ()
    };

    // -- Save Task
    match Task::save(&task, &db).await {
        Ok(_) => (),
        Err(_e) => {
            let data = json!({ "error": "Failed to save task to database" });
            let response = SysResponse { data };

            return (Status::InternalServerError, Json(response));
        }
    }

    // -> Send Task Created Response
    let data = json!({ "task_id": task.id });
    let response = SysResponse { data };

    (Status::Created, Json(response))
}

pub async fn create_payment(
    payment_request: Json<PaymentCreate<'_>>,
    db: &State<Database>,
) -> WebResponse {
    let request = payment_request.into_inner();
    let db = &db.db;
    let task = {
        let fetch_task = match Task::fetch_one_by_id(request.task_id, &db).await {
            Ok(fetch) => fetch,
            Err(_) => {
                let data = json!({ "error": "Database query failed" });
                let response = SysResponse { data };

                return (Status::InternalServerError, Json(response));
            }
        };

        let exists = match fetch_task {
            Some(task) => task,
            None => {
                let data = json!({ "error": "Task does not exist" });
                let response = SysResponse { data };

                return (Status::NotFound, Json(response));
            }
        };

        exists
    };

    let new_payment = Payment::new(request, task.price);

    match new_payment.save(&db).await {
        Ok(_) => (),
        Err(_) => {
            let data = json!({ "error": "Failed to save payment to database" });
            let response = SysResponse { data };

            return (Status::InternalServerError, Json(response));
        }
    }

    // -> paymentid
    let data = json!({ "payment_id": new_payment.id });
    let response = SysResponse { data };

    return (Status::Created, Json(response))
}

pub async fn receive_payment(
    payment_request: Json<PaymentReceive<'_>>,
    db: &State<Database>,
) -> WebResponse {
    let request = payment_request.into_inner();
    let db = &db.db;

    let mut payment = {
        let fetch_payment = match Payment::fetch_one_by_id(request.payment_id, &db).await {
            Ok(found) => found,
            Err(_) => {
                let data = json!({ "error": "Failed to fetch payment" });
                let response = SysResponse { data };

                return (Status::InternalServerError, Json(response));
            }
        };

        let exists = match fetch_payment {
            Some(payment) => payment,
            None => {
                let data = json!({ "error": "Payment does not exist" });
                let response = SysResponse { data };

                return (Status::NotFound, Json(response));
            }
        };

        exists
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

    let _confirm_result = match crate::handlers::payment::confirm_transaction(&signature, &db).await {
        Ok(_) => (),
        Err(_) => {
            let data = json!({ "error": "Invalid signature" });
            let response = SysResponse { data };
            
            return (Status::BadRequest, Json(response));
        }
    };

    // Set Payment to Success
    payment.success = true;
    let _confirm_payment = match payment.confirm_payment(&db).await {
        Ok(_) => (),
        Err(_) => {
            let data = json!({ "error": "Failed to update payment" });
            let response = SysResponse { data };
            
            return (Status::BadRequest, Json(response));
        }
    };

    let mut task = {
        let fetch_task = match Task::fetch_one_by_id(&payment.task_id, &db).await {
            Ok(fetch) => fetch,
            Err(_) => {
                let data = json!({ "error": "Database query failed" });
                let response = SysResponse { data };

                return (Status::InternalServerError, Json(response));
            }
        };

        let exists = match fetch_task {
            Some(task) => task,
            None => {
                let data = json!({ "error": "Task does not exist" });
                let response = SysResponse { data };

                return (Status::NotFound, Json(response));
            }
        };

        exists
    };

    let _update_metadata = match handle_update(&task.mint_address).await {
        Ok(_) => {
            ()
        },
        Err(e) => return e
    };

    task.success = true;
    let _update_task = match task.update_task(&db).await {
        Ok(_) => (),
        Err(_) => {
            let data = json!({ "error": "Failed to update task" });
            let response = SysResponse { data };

            return (Status::NotFound, Json(response));
        }
    };

    let data = json!({ "error": "Task does not exist" });
    let response = SysResponse { data };

    return (Status::NotFound, Json(response));
}
