#[derive(PartialEq,Eq)] enum Method { GET, HEAD, POST, PUT, DELETE }
#[derive(Debug)] enum Kind { Login, Registration, Confirmation, Oauth, Credential }
fn kind(method: &Method, path: &str) -> Option<Kind> {
    // Axum GET routes also invoke their handler for HEAD. Count that method before it can
    // reach an OAuth provider exchange.
    if *method == Method::HEAD {
        return kind(&Method::GET, path);
    }
    match (method, path) {
        (&Method::POST, "/login" | "/api/user/login" | "/api/json/user/login") => Some(Kind::Login),
        (&Method::POST, "/register" | "/api/user/register" | "/api/json/user/register") => {
            Some(Kind::Registration)
        }
        (&Method::POST, "/api/user/register/confirm" | "/api/json/user/register/confirm") => {
            Some(Kind::Confirmation)
        }
        (
            &Method::GET,
            "/auth/google/register"
            | "/auth/google/login"
            | "/auth/google/connect"
            | "/auth/google/callback",
        )
        | (&Method::GET, "/auth/apple/register" | "/auth/apple/login" | "/auth/apple/connect")
        | (&Method::POST, "/auth/apple/callback") => Some(Kind::Oauth),
        (&Method::PUT, "/ui/profile" | "/ui/profile/password" | "/ui/profile/password/setup")
        | (&Method::POST | &Method::DELETE, "/ui/profile/changes/{kind}") => Some(Kind::Credential),
        _ => None,
    }
}
fn main(){for (method,path) in [(Method::POST,"/ui/profile/vault/apps/{app}/connect"),(Method::GET,"/auth/integrations/{provider}/callback"),(Method::HEAD,"/auth/integrations/{provider}/callback"),(Method::GET,"/auth/google/callback")] { println!("{} {:?}",path,kind(&method,path));}}