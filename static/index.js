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
player_state_changes.addEventListener("pipeline_state", message => { pipeline_state.innerText = message.data; });
player_state_changes.addEventListener("volume", message => { volume_slider.value = parseInt(message.data); });
player_state_changes.addEventListener("buffering", message => { buffering_bar.value = parseInt(message.data); });
player_state_changes.addEventListener("current_track", message => {
    let current_track = JSON.parse(message.data);
    if (current_track === null) {
        current_track_tags.style.display = "none";
    } else {
        current_track_tags.style.display = "";
        current_track_title.innerText = current_track.title;
        current_track_artist.innerText = current_track.artist;
        current_track_album.innerText = current_track.album;
        current_track_genre.innerText = current_track.genre;
        if (current_track.image === null) {
            current_track_image.style.display = "none";
        } else {
            current_track_image.style.display = "";
            current_track_image.src = current_track.image;
        }
    }
});

