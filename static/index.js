let lost_connection_message = document.getElementById("lost_connection");

let pipeline_state = document.getElementById("pipeline_state");
let volume_slider = document.getElementById("set_volume");
let buffering_bar = document.getElementById("buffering");
let current_track_tags = document.getElementById("track_tags");
let current_track_title = document.getElementById("track_title");
let current_track_artist = document.getElementById("track_artist");
let current_track_album = document.getElementById("track_album");
let current_track_genre = document.getElementById("track_genre");
let current_track_image = document.getElementById("track_image");

volume_slider.addEventListener("input", ev => post("/volume", ev.target.value));

let player_state_display = document.getElementById("player_state");

let player_state_changes = new EventSource("/state_changes");
player_state_changes.addEventListener("open", () => { lost_connection_message.style.display = "none"; });
player_state_changes.addEventListener("error", () => { lost_connection_message.style.display = ""; });

function apply_new_value(value, action) {
    if (value !== null && value !== undefined) {
        action(value)
    }
}

player_state_changes.addEventListener("new_state", message => {
    let new_state = JSON.parse(message.data);

    console.log(new_state);

    apply_new_value(new_state.pipeline_state, value => pipeline_state.innerText = value);
    apply_new_value(new_state.current_track_tags, tags => {
        if (tags.tags === null) {
            current_track_tags.style.display = "none";
        } else {
            current_track_tags.style.display = "";
            current_track_title.innerText = tags.tags.title;
            current_track_artist.innerText = tags.tags.artist;
            current_track_album.innerText = tags.tags.album;
            current_track_genre.innerText = tags.tags.genre;
            if (tags.tags.image === null) {
                current_track_image.style.display = "none";
            } else {
                current_track_image.style.display = "";
                current_track_image.src = tags.tags.image;
            }
        }
    });
    apply_new_value(new_state.volume, value => volume_slider.value = value);
    apply_new_value(new_state.buffering, value => buffering_bar.value = value);
});

