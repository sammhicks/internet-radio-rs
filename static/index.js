let lost_connection_message = document.getElementById("lost_connection");

let pipeline_state = document.getElementById("pipeline_state");
let volume_slider = document.getElementById("set_volume");
let buffering_bar = document.getElementById("buffering");

volume_slider.addEventListener("input", (ev) => fetch("/set_volume/" + ev.target.value));

let player_state_display = document.getElementById("player_state");

let player_state_changes = new EventSource("/state_changes");
player_state_changes.addEventListener("open", () => { lost_connection_message.style.display = "none"; });
player_state_changes.addEventListener("error", () => { lost_connection_message.style.display = ""; });
player_state_changes.addEventListener("pipeline_state", (message) => { pipeline_state.innerText = message.data; });
player_state_changes.addEventListener("volume", (message) => { volume_slider.value = parseInt(message.data); });
player_state_changes.addEventListener("buffering", (message) => { buffering_bar.value = parseInt(message.data); });
