use actix::prelude::*;
use actix_web::{web, App, HttpServer, HttpResponse, Error, HttpRequest};
use actix_web_actors::ws;
use actix_cors::Cors;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use mongodb::{bson::{self, doc}, options::ClientOptions, Client, Collection};
use std::sync::Arc;

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Room {
    id: Uuid,
    name: String,
    creator: String,
    users: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct User {
    username: String,
    password: String,
}

struct AppState {
    users_collection: Collection<User>,
    rooms_collection: Collection<Room>,
}

#[derive(Debug, Deserialize)]
struct RegisterRequest {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct CreateRoomRequest {
    name: String,
    creator: String,
}

#[derive(Deserialize)]
struct AddUserRequest {
    room_id: String,
    username: String,
}

async fn register(
    data: web::Data<Arc<AppState>>,
    req: web::Json<RegisterRequest>,
) -> HttpResponse {
    let new_user = User {
        username: req.username.clone(),
        password: req.password.clone(),
    };

    let result = data.users_collection.insert_one(new_user).await;
    match result {
        Ok(_) => HttpResponse::Ok().body("User registered successfully"),
        Err(err) => {
            log::error!("Failed to register user: {:?}", err);
            HttpResponse::InternalServerError().body("Failed to register user")
        }
    }
}

async fn login(
    data: web::Data<Arc<AppState>>,
    req: web::Json<LoginRequest>,
) -> HttpResponse {
    let filter = doc! {"username": &req.username, "password": &req.password};
    let user = data.users_collection.find_one(filter).await;
    match user {
        Ok(Some(_)) => HttpResponse::Ok().body("Login successful"),
        Ok(None) => HttpResponse::Unauthorized().body("Invalid username or password"),
        Err(err) => {
            log::error!("Failed to log in: {:?}", err);
            HttpResponse::InternalServerError().body("Failed to log in")
        }
    }
}

async fn create_room(
    data: web::Data<Arc<AppState>>,
    req: web::Json<CreateRoomRequest>,
) -> HttpResponse {
    let new_room = Room {
        id: Uuid::new_v4(),
        name: req.name.clone(),
        creator: req.creator.clone(),
        users: vec![req.creator.clone()],
    };

    let result = data.rooms_collection.insert_one(&new_room).await;
    match result {
        Ok(_) => HttpResponse::Ok().json(new_room),
        Err(err) => {
            log::error!("Failed to create room: {:?}", err);
            HttpResponse::InternalServerError().body("Failed to create room")
        }
    }
}

async fn add_user(
    data: web::Data<Arc<AppState>>,
    req: web::Json<AddUserRequest>,
) -> HttpResponse {
    let room_id = Uuid::parse_str(&req.room_id).unwrap_or(Uuid::nil());
    let filter = doc! {"id": bson::to_bson(&room_id).unwrap()};
    let update = doc! {"$push": {"users": &req.username}};

    let result = data.rooms_collection.update_one(filter, update).await;
    match result {
        Ok(update_result) if update_result.matched_count > 0 => HttpResponse::Ok().body("User added to room"),
        Ok(_) => HttpResponse::NotFound().body("Room not found"),
        Err(err) => {
            log::error!("Failed to add user to room: {:?}", err);
            HttpResponse::InternalServerError().body("Failed to add user to room")
        }
    }
}

async fn list_rooms(data: web::Data<Arc<AppState>>) -> HttpResponse {
    let cursor = data.rooms_collection.find(doc! {}).await;
    match cursor {
        Ok(mut rooms_cursor) => {
            let mut rooms = vec![];
            while let Some(room) = rooms_cursor.next().await {
                match room {
                    Ok(room_doc) => rooms.push(room_doc),
                    Err(err) => {
                        log::error!("Error reading room: {:?}", err);
                        return HttpResponse::InternalServerError().body("Failed to fetch rooms");
                    }
                }
            }
            HttpResponse::Ok().json(rooms)
        }
        Err(err) => {
            log::error!("Failed to fetch rooms: {:?}", err);
            HttpResponse::InternalServerError().body("Failed to fetch rooms")
        }
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    let mongo_uri = "mongodb://localhost:27017";
    let client_options = ClientOptions::parse(mongo_uri).await.unwrap();
    let client = Client::with_options(client_options).unwrap();

    let db = client.database("chat_app");
    let users_collection = db.collection::<User>("users");
    let rooms_collection = db.collection::<Room>("rooms");

    let app_state = Arc::new(AppState {
        users_collection,
        rooms_collection,
    });

    HttpServer::new(move || {
        App::new()
            .wrap(Cors::default().allow_any_origin().allow_any_method().allow_any_header())
            .app_data(web::Data::new(app_state.clone()))
            .route("/register", web::post().to(register))
            .route("/login", web::post().to(login))
            .route("/create_room", web::post().to(create_room))
            .route("/add_user", web::post().to(add_user))
            .route("/list_rooms", web::get().to(list_rooms))
    })
        .bind("127.0.0.1:8080")?
        .run()
        .await
}
