use maud::{html, Markup, DOCTYPE};

use crate::collection::{Collection, Movie};

pub const MISSING_POSTER: &[u8] = include_bytes!("static/missing_poster.jpg");

pub fn index(collection: &Collection) -> Markup {
    let mut sorted_movies = collection.movies.values().collect::<Vec<_>>();
    sorted_movies.sort_by_key(|m| &m.name);
    html! {
        (DOCTYPE)
        html {
            head {
                title { "Movie Tagger" }
                link rel="stylesheet" href="/static/reset.css";
                link rel="stylesheet" href="/static/pico.min.css";
                link rel="stylesheet" href="/static/main.css";
                script src="/static/htmx.min.js" {}
            }
            body {
                header {
                    h1 { "Movie Tagger" }
                    form method="post" action="/reload" {
                        button type="submit" { "Reload" }
                    }
                }
                main {
                    div #movie-list {
                        @for m in sorted_movies {
                            (movie(collection, m))
                        }
                    }
                }
                footer {}
            }
        }
    }
}

pub fn movie(collection: &Collection, movie: &Movie) -> Markup {
    let tags = collection.tags.iter().map(|(name, tag_movies)| {
        let mut tag_classes = vec!["tag"];
        if !tag_movies.contains(&movie.hash) { tag_classes.push("secondary") };
        html! {
            button
                hx-post=(format!("/movie/{}/tag/{}", movie.id(), name))
                hx-target=(format!("#movie-{}", movie.id()))
                hx-swap="outerHTML"
                class=(tag_classes.join(" "))
                { (name) }
        }
    });
    let poster_url = format!("/movie/{}/poster.jpg", movie.id());
    html! {
        article .movie id=(format!("movie-{}", movie.id())) {
            header { h2 { (movie.name) } }
            img src=(poster_url) alt=(format!("{} poster", movie.name)) {}
            footer .tags { @for tag in tags { (tag) } }
        }
    }
}
