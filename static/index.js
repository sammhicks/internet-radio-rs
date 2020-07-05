document.getElementById("play_pause").addEventListener("click", () => fetch("/play_pause"));
document.getElementById("set_volume").addEventListener("input", (ev) => fetch("/set_volume/" + ev.target.value));

let player_state_display = document.getElementById("player_state");

let player_state_changes = new EventSource("/state_changes");
player_state_changes.addEventListener("message", (message) => { player_state_display.innerText = message.data; });
player_state_changes.addEventListener("error", console.log);
