use maud::{html, Markup, DOCTYPE};

use crate::collection::{Collection, Movie, Error};
use crate::jellyfin_api::{MediaFolders, User};

pub const MISSING_POSTER: &[u8] = include_bytes!("static/missing_poster.jpg");

#[derive(Debug, Default, Clone)]
struct PageOptions {
    controls: Option<Markup>,
    footer: Option<Markup>,
}

pub fn page(title: &str, content: Markup, options: PageOptions) -> Markup {
    html! {
        (DOCTYPE)
        html {
            head {
                title { (title) }
                link rel="stylesheet" href="/static/reset.css";
                link rel="stylesheet" href="/static/pico.min.css";
                link rel="stylesheet" href="/static/main.css";
                script src="/static/htmx.min.js" {}
            }
            body {
                header {
                    h1 { (title) }
                    a href="/" { "Movie Tagger" }
                    a href="/user-libraries" { "User Libraries" }
                    @if let Some(c) = options.controls { (c) } @else { div {} }
                }
                main { (content) }
                footer { @if let Some(f) = options.footer { (f) } }
            }
        }
    }
}

pub fn index(collection: &Collection) -> Markup {
    let mut sorted_movies = collection.movies.values().collect::<Vec<_>>();
    sorted_movies.sort_by_key(|m| &m.name);
    let controls = html! {
        form method="post" action="/reload" {
            button type="submit" { "Reload" }
        }
    };
    let content = html! {
        div #movie-list {
            @for m in sorted_movies {
                (movie(collection, m))
            }
        }
    };
    page("Movie Tagger", content, PageOptions { controls: Some(controls), footer: None })
}

pub fn movie(collection: &Collection, movie: &Movie) -> Markup {
    let tags = collection.tags.iter().map(|(name, tag_movies)| {
        let mut tag_classes = vec!["tag"];
        if !tag_movies.contains(&movie.hash) { tag_classes.push("secondary") };
        html! {
            button
                hx-post=(format!("/movie/{}/tag/{}", movie.id(), name))
                hx-target={"#movie-" (movie.id())}
                hx-swap="outerHTML"
                class=(tag_classes.join(" "))
                { (name) }
        }
    });
    let poster_url = format!("/movie/{}/poster.jpg", movie.id());
    html! {
        article .movie id={"movie-" (movie.id())} {
            header { h2 { (movie.name) } }
            img src=(poster_url) alt=(format!("{} poster", movie.name)) {}
            footer .tags { @for tag in tags { (tag) } }
        }
    }
}

pub fn user_libraries_page(users: &[User], folders: &[MediaFolders]) -> Result<Markup, Error> {
    let content = html! {
        @for user in users {
            (user_libraries_entry(user, folders)?)
        }
    };
    Ok(page("User Libraries", content, Default::default()))
}

pub fn user_libraries_entry(user: &User, folders: &[MediaFolders]) -> Result<Markup, Error> {
    let user_folders: Vec<String> = user.enabled_folders()?;
    let folder_buttons = folders.iter().map(|folder| {
        let mut classes = vec![];
        if !user_folders.contains(&folder.id) { classes.push("secondary") };
        let input_id = format!("check-{}-{}", user.id, folder.id);
        html! {
            button
                id=(input_id)
                class=(classes.join(" "))
                hx-post=(format!("/user/{}/library/{}", user.id, folder.id))
                hx-target=(format!("#user-{}", user.id))
                hx-swap="outerHTML"
                { (folder.name) }
        }
    });
    Ok(html! {
        div .user-library.grid id=(format!("user-{}", user.id)) {
            h2 { (user.name) }
            @for folder in folder_buttons { (folder) }
        }
    })
}
