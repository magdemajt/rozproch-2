use std::collections::HashMap;
use std::path::{Path, PathBuf};
use actix_cors::Cors;
use futures::try_join;
use actix_web::{web, App, HttpServer, Responder, get, HttpResponse, HttpRequest};
use rand::prelude::SliceRandom;
use rand::SeedableRng;
use serde_json::{json};
use serde::{Deserialize, Serialize};
use actix_files as fs;
use actix_web::http::StatusCode;
use actix_web_httpauth::extractors::basic::{self, BasicAuth};

const USERNAME: &str = "admin";
const PASSWORD: &str = "admin";

fn check_auth(
    auth: BasicAuth,
) -> bool {
    if auth.password().is_none() {
        return false;
    }
    if auth.user_id() == USERNAME && auth.password().unwrap() == PASSWORD {
        true
    } else {
        false
    }
}

#[derive(Deserialize)]
struct QuoteOfDayBody {
    affirmation: String,
}

#[get("/quote_of_the_day")]
async fn quote_of_the_day() -> impl Responder {
    let body = reqwest::get("https://www.affirmations.dev/").await;
    if let Err(_) = body {
        return HttpResponse::BadGateway().finish();
    }

    let body = body.unwrap().text().await;
    if let Err(_) = body {
        return HttpResponse::BadGateway().finish();
    }
    let body = body.unwrap();

    let quote: serde_json::Result<QuoteOfDayBody> = serde_json::from_str(&body);
    if let Err(_) = quote {
        return HttpResponse::BadGateway().finish();
    }

    let quote = quote.unwrap();

    let quote_with_desc = format!("Quote of the day: {}", quote.affirmation);

    HttpResponse::Ok().body(quote_with_desc)
}


#[derive(Deserialize, Debug)]
struct Book {
    title: String,
    formats: HashMap<String, String>,
}

#[derive(Deserialize)]
struct PoetryAuthorResponse {
    authors: Vec<String>,
}

#[derive(Deserialize)]
struct WolneLekturyAuthor {
    name: String,
}

#[derive(Deserialize)]
struct AuthorQuery {
    limit: Option<u64>,
    offset: Option<u64>,
    name: Option<String>,
}

#[get("/author")]
async fn get_all_authors(query: web::Query<AuthorQuery>, user: BasicAuth) -> impl Responder {

    if !check_auth(user) {
        return HttpResponse::Unauthorized().finish();
    }

    if let Some(limit) = query.limit {
        if limit > 100 {
            return HttpResponse::BadRequest().body("Limit cannot be greater than 100");
        }
    }

    if let Some(offset) = query.offset {
        if offset > 1000 {
            return HttpResponse::BadRequest().body("Offset cannot be greater than 1000");
        }
    }

    let wolne_lektury_req = reqwest::get("https://wolnelektury.pl/api/authors/");
    let poetry_db_req = reqwest::get("https://poetrydb.org/author");

    let result = try_join!(wolne_lektury_req, poetry_db_req);

    if let Err(_) = result {
        return HttpResponse::BadGateway().finish();
    }

    let (wolne_lektury_req, poetry_db_req) = result.unwrap();

    let wolne_lektury_body = wolne_lektury_req.text();
    let poetry_db_body = poetry_db_req.text();

    let result = try_join!(wolne_lektury_body, poetry_db_body);

    if let Err(_) = result {
        return HttpResponse::BadGateway().finish();
    }

    let (wolne_lektury_body, poetry_db_body) = result.unwrap();


    let poetry_db_authors: serde_json::Result<PoetryAuthorResponse> = serde_json::from_str(&poetry_db_body);

    if let Err(_) = poetry_db_authors {
        return HttpResponse::BadGateway().finish();
    }

    let poetry_db_authors = poetry_db_authors.unwrap().authors;

    let wolne_lektury_authors: serde_json::Result<Vec<WolneLekturyAuthor>> = serde_json::from_str(&wolne_lektury_body);

    if let Err(_) = wolne_lektury_authors {
        return HttpResponse::BadGateway().finish();
    }

    let wolne_lektury_authors = wolne_lektury_authors.unwrap();

    let mut authors = Vec::new();


    for author in wolne_lektury_authors {
        authors.push(author.name);
    }

    for author in poetry_db_authors {
        if !authors.contains(&author) {
            authors.push(author);
        }
    }


    if let Some(name) = &query.name {
        authors = authors.into_iter().filter(|author| author.contains(name)).collect();
    }

    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(authors.len() as u64);


    let authors = authors.into_iter().skip(offset as usize)
        .take(limit as usize)
        .collect::<Vec<String>>();

    HttpResponse::Ok().json(authors)
}


#[derive(Deserialize, Serialize)]
struct PoetryDbResponse {
    title: String,
    author: String,
    lines: Vec<String>,
    linecount: String,
}

async fn get_poems_for_author(author: String) -> Result<Vec<PoetryDbResponse>, StatusCode> {
    let request = reqwest::get(format!("https://poetrydb.org/author/{}", author).as_str()).await;

    if let Err(err) = request {
        match err.status() {
            Some(reqwest::StatusCode::NOT_FOUND) => return Err(StatusCode::NOT_FOUND),
            Some(_) => return Err(StatusCode::BAD_GATEWAY),
            None => return Err(StatusCode::BAD_GATEWAY),
        }
    }

    let request = request.unwrap().text().await;

    if let Err(_) = request {
        return Err(StatusCode::BAD_GATEWAY);
    }

    let request = request.unwrap();


    let poems: serde_json::Result<Vec<PoetryDbResponse>> = serde_json::from_str(&request);

    if let Err(_) = poems {
        return Err(StatusCode::BAD_GATEWAY);
    }

    let poems = poems.unwrap();
    Ok(poems)
}

#[derive(Deserialize)]
struct PoemQuery {
    sort: Option<String>,
    search: Option<String>,
    sort_order: Option<String>,
}

// author poems
#[get("/author/{author}/poems")]
async fn get_author_poems(path: web::Path<(String, )>, query: web::Query<PoemQuery>, user: BasicAuth) -> impl Responder {
    if !check_auth(user) {
        return HttpResponse::Unauthorized().finish();
    }

    let author = path.into_inner().0;

    let poems = get_poems_for_author(author).await;

    if let Err(err) = poems {
        return HttpResponse::build(err).finish();
    }

    let poems = poems.unwrap();

    let sort_order = &query.sort_order;

    let sort_order = match sort_order {
        Some(sort_order) => {
            if sort_order == "asc" {
                "asc"
            } else {
                "desc"
            }
        }
        None => "asc",
    };

    let mut filtered_poems = match &query.search {
        Some(search) => {
            let search = search.to_lowercase();
            poems.into_iter().filter(|poem| {
                let title = poem.title.to_lowercase();
                title.contains(&search)
            }).collect::<Vec<PoetryDbResponse>>()
        }
        None => poems,
    };

    let sort = &query.sort;

    let poems = match sort {
        Some(sort) => {
            if sort == "title" {
                if sort_order == "asc" {
                    filtered_poems.sort_by(|a, b| a.title.cmp(&b.title));
                } else {
                    filtered_poems.sort_by(|a, b| b.title.cmp(&a.title));
                }
            } else if sort == "author" {
                if sort_order == "asc" {
                    filtered_poems.sort_by(|a, b| a.author.cmp(&b.author));
                } else {
                    filtered_poems.sort_by(|a, b| b.author.cmp(&a.author));
                }
            } else if sort == "linecount" {
                if sort_order == "asc" {
                    filtered_poems.sort_by(|a, b| a.linecount.cmp(&b.linecount));
                } else {
                    filtered_poems.sort_by(|a, b| b.linecount.cmp(&a.linecount));
                }
            }
            filtered_poems
        }
        None => filtered_poems,
    };

    HttpResponse::Ok().json(json!(poems))
}

#[get("/author/{author}/poems/word_count")]
async fn get_author_poems_word_count(path: web::Path<(String, )>, user: BasicAuth) -> impl Responder {
    if !check_auth(user) {
        return HttpResponse::Unauthorized().finish();
    }
    let author = path.into_inner().0;

    let poems = get_poems_for_author(author).await;

    if let Err(code) = poems {
        return HttpResponse::build(code).finish();
    }

    let poems = poems.unwrap();

    let mut word_map = HashMap::new();

    for poem in poems {
        for line in poem.lines {
            let words = line.split(" ").collect::<Vec<&str>>();

            for word in words {
                let word = word.to_lowercase();

                if word_map.contains_key(&word) {
                    let count = word_map.get(&word).unwrap();
                    word_map.insert(word, count + 1);
                } else {
                    word_map.insert(word, 1);
                }
            }
        }
    }

    let mut word_map = word_map.into_iter().collect::<Vec<(String, u64)>>();

    word_map.sort_by(|a, b| b.1.cmp(&a.1));

    HttpResponse::Ok().json(json!(word_map))
}


#[derive(Deserialize)]
struct RandomPoemQuery {
    seed: Option<u64>,
}

// random author poem
#[get("/author/{author}/poems/random")]
async fn get_random_author_poem(path: web::Path<(String, )>, query: web::Query<RandomPoemQuery>, user: BasicAuth) -> impl Responder {
    if !check_auth(user) {
        return HttpResponse::Unauthorized().finish();
    }
    let author = path.into_inner().0;

    let poems = get_poems_for_author(author).await;

    if let Err(code) = poems {
        return HttpResponse::build(code).finish();
    }

    let random_seed = query.seed.unwrap_or(rand::random::<u64>());

    let mut rng = rand::rngs::StdRng::seed_from_u64(random_seed);

    let poems = poems.unwrap();

    let poem = poems.choose(&mut rng);

    HttpResponse::Ok().json(json!(poem))
}


#[derive(Deserialize, Debug)]
struct ProjectGutenbergBooksResponse {
    results: Vec<Book>,
}

#[derive(Deserialize)]
struct WolneLekturyBook {
    kind: String,
    title: String,
    href: String,
}

// books

#[derive(Deserialize)]
struct BooksQuery {
    search: Option<String>,
    page: Option<u64>,
    topic: Option<String>,
}

const BOOKS_PER_PAGE: u64 = 10;

#[derive(Deserialize)]
struct ShortenUrlResponse {
    result_url: String,
}

async fn shorten_url(url: &str) -> Option<String> {
    let cleanuri_url = format!("https://cleanuri.com/api/v1/shorten");
    let client = reqwest::Client::new();
    let res = client.post(&cleanuri_url)
        .form(&[("url", url)])
        .send()
        .await;
    if let Err(_) = res {
        return None;
    }
    let res = res.unwrap();

    let res = res.text().await;

    if let Err(_) = res {
        return None;
    }

    let res = res.unwrap();

    let res: serde_json::Result<ShortenUrlResponse> = serde_json::from_str(&res);

    if let Err(_) = res {
        return None;
    }

    let res = res.unwrap();

    return Some(res.result_url);
}

async fn get_gutendex_books(page: &u64, search: &Option<String>, topic: &Option<String>) -> Result<ProjectGutenbergBooksResponse, StatusCode> {
    let mut base_gutendex_url = "https://gutendex.com/books/?sort=descending".to_string();

    if let Some(search) = search {
        base_gutendex_url = format!("{}&search={}", base_gutendex_url, search.to_lowercase());
    }

    if let Some(topic) = topic {
        base_gutendex_url = format!("{}&topic={}", base_gutendex_url, topic);
    }

    let gutendex_url = format!("{}&page={}", base_gutendex_url, page);

    let gutendex_req = reqwest::get(&gutendex_url).await;

    if let Err(err) = gutendex_req {
        if err.status() == Some(StatusCode::NOT_FOUND) {
            return Err(StatusCode::NOT_FOUND);
        }
        return Err(StatusCode::BAD_GATEWAY);
    }

    let gutendex_req = gutendex_req.unwrap();

    let gutendex_req = gutendex_req.text().await;

    if let Err(_) = gutendex_req {
        return Err(StatusCode::BAD_GATEWAY);
    }

    let gutendex_req = gutendex_req.unwrap();

    let gutendex_req: serde_json::Result<ProjectGutenbergBooksResponse> = serde_json::from_str(&gutendex_req);

    if let Err(_) = gutendex_req {
        return Err(StatusCode::BAD_GATEWAY);
    }

    let gutendex_req = gutendex_req.unwrap();

    return Ok(gutendex_req);
}

async fn get_wolne_lektury_books(page: &u64, search: &Option<String>, topic: &Option<String>) -> Result<Vec<WolneLekturyBook>, StatusCode> {
    let wolne_lektury_req = reqwest::get("https://wolnelektury.pl/api/books/").await;

    if let Err(err) = wolne_lektury_req {
        if err.status() == Some(StatusCode::NOT_FOUND) {
            return Err(StatusCode::NOT_FOUND);
        }
        return Err(StatusCode::BAD_GATEWAY);
    }

    let wolne_lektury_req = wolne_lektury_req.unwrap();

    let wolne_lektury_req = wolne_lektury_req.text().await;

    if let Err(_) = wolne_lektury_req {
        return Err(StatusCode::BAD_GATEWAY);
    }

    let wolne_lektury_req = wolne_lektury_req.unwrap();

    let wolne_lektury_req: serde_json::Result<Vec<WolneLekturyBook>> = serde_json::from_str(&wolne_lektury_req);

    if let Err(_) = wolne_lektury_req {
        return Err(StatusCode::BAD_GATEWAY);
    }

    let wolne_lektury_books = wolne_lektury_req.unwrap();

    let wolne_lektury_books = if let Some(search) = search {
        let wolne_lektury_books = wolne_lektury_books.into_iter().filter(|book| {
            book.title.to_lowercase().contains(&search.to_lowercase())
        }).collect::<Vec<WolneLekturyBook>>();
        wolne_lektury_books
    } else {
        wolne_lektury_books
    };

    let wolne_lektury_books = if let Some(topic) = topic {
        let wolne_lektury_books = wolne_lektury_books.into_iter().filter(|book| {
            book.kind.to_lowercase().contains(&topic.to_lowercase())
        }).collect::<Vec<WolneLekturyBook>>();
        wolne_lektury_books
    } else {
        wolne_lektury_books
    };

    // sort wolne lektury by title
    let mut wolne_lektury_books = wolne_lektury_books.into_iter().collect::<Vec<WolneLekturyBook>>();
    wolne_lektury_books.sort_by(|a, b| a.title.cmp(&b.title));

    // take page of wolne lektury books
    let wolne_lektury_books = wolne_lektury_books.into_iter().skip(((page - 1) * BOOKS_PER_PAGE) as usize).take(BOOKS_PER_PAGE as usize).collect::<Vec<WolneLekturyBook>>();

    return Ok(wolne_lektury_books);
}

#[get("/books")]
async fn get_books(query: web::Query<BooksQuery>, user: BasicAuth) -> impl Responder {
    if !check_auth(user) {
        return HttpResponse::Unauthorized().finish();
    }
    let search = &query.search;
    let page = &query.page;
    let topic = &query.topic;

    let page = match page {
        Some(page) => page,
        None => &1,
    };

    let gutendex_books = get_gutendex_books(page, search, topic);
    let wolne_lektury_books = get_wolne_lektury_books(page, search, topic);

    let result = futures::join!(gutendex_books, wolne_lektury_books);

    let (gutendex_books, wolne_lektury_books) = result;

    if let Err(code) = gutendex_books {
        if code == StatusCode::BAD_GATEWAY {
            return HttpResponse::BadGateway().finish();
        }
    }

    if let Err(code) = wolne_lektury_books {
        if code == StatusCode::BAD_GATEWAY {
            return HttpResponse::BadGateway().finish();
        }
    }

    if let Err(code_guten) = gutendex_books {
        if code_guten == StatusCode::NOT_FOUND {
            if let Err(code_wolne) = wolne_lektury_books {
                if code_wolne == StatusCode::NOT_FOUND {
                    return HttpResponse::NotFound().finish();
                }
            }
        }
    }

    let gutendex_books = gutendex_books.unwrap();
    let wolne_lektury_books = wolne_lektury_books.unwrap();


    let mut books = Vec::new();

    for book in gutendex_books.results {
        books.push(book.title);
    }

    for book in wolne_lektury_books {
        books.push(book.title);
    }

    HttpResponse::Ok().json(books)
}

#[get("/books/{book_title}")]
async fn get_book(path: web::Path<String>, user: BasicAuth) -> impl Responder {
    if !check_auth(user) {
        return HttpResponse::Unauthorized().finish();
    }
    let book_title = path.into_inner();
    let some_title = Some(book_title.clone());
    let gutendex_books = get_gutendex_books(&1, &some_title, &None);
    let wolne_lektury_books = get_wolne_lektury_books(&1, &some_title, &None);

    let result = futures::join!(gutendex_books, wolne_lektury_books);

    let (gutendex_books, wolne_lektury_books) = result;

    if let Err(code) = gutendex_books {
        if code == StatusCode::BAD_GATEWAY {
            return HttpResponse::BadGateway().finish();
        }
    }

    if let Err(code) = wolne_lektury_books {
        if code == StatusCode::BAD_GATEWAY {
            return HttpResponse::BadGateway().finish();
        }
    }

    if let Err(code_guten) = gutendex_books {
        if code_guten == StatusCode::NOT_FOUND {
            if let Err(code_wolne) = wolne_lektury_books {
                if code_wolne == StatusCode::NOT_FOUND {
                    return HttpResponse::NotFound().finish();
                }
            }
        }
    }

    let gutendex_books = gutendex_books.unwrap();
    let wolne_lektury_books = wolne_lektury_books.unwrap();

    let gutendex_book = gutendex_books.results.into_iter().find(|book| book.title == book_title);
    let wolne_lektury_book = wolne_lektury_books.into_iter().find(|book| book.title == book_title);

    if gutendex_book.is_none() && wolne_lektury_book.is_none() {
        return HttpResponse::NotFound().finish();
    }

    let mut urls = Vec::new();

    if let Some(gutendex_book) = gutendex_book {
        let url = gutendex_book.formats.into_values().next();
        if let Some(url) = url {
            let shorten_url = shorten_url(url.as_str()).await;
            if let Some(shorten_url) = shorten_url {
                urls.push(shorten_url);
            }
        }
    }

    if let Some(wolne_lektury_book) = wolne_lektury_book {
        let url = wolne_lektury_book.href.as_str();
        let shorten_url = shorten_url(url).await;
        if let Some(shorten_url) = shorten_url {
            urls.push(shorten_url);
        }
    }

    HttpResponse::Ok().json(urls)
}

// add auth

// get static resources
async fn get_static_resource(req: HttpRequest) -> actix_web::Result<fs::NamedFile> {
    let path: PathBuf = req.match_info().query("filename").parse().unwrap();
    let path = Path::new("src/static/").join(path);
    Ok(fs::NamedFile::open(path)?)
}

async fn index_file() -> actix_web::Result<fs::NamedFile> {
    Ok(fs::NamedFile::open("src/static/index.html")?)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {

    HttpServer::new(move || {

        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header()
            .supports_credentials();

        App::new()
            .wrap(cors)
            .app_data(basic::Config::default().realm("Protected"))
            .service(quote_of_the_day)
            .service(get_all_authors)
            .service(get_author_poems)
            .service(get_random_author_poem)
            .service(get_books)
            .service(get_book)
            .route("/static/{filename:.*}", web::get().to(get_static_resource))
            .route("/", web::get().to(index_file))
    })
        .bind(("127.0.0.1", 8080))?
        .run()
        .await
}