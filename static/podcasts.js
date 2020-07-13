const LOCAL_STORAGE_PODCASTS_KEY = "podcasts";

function getChildText(element, child_name) {
    const child = element.getElementsByTagName(child_name)[0];
    return child ? child.textContent : undefined;
}

const podcasts_select = document.getElementById("podcasts_select");
const new_podcast = document.getElementById("new_podcast");

const podcast_title = document.getElementById("podcast_title");
const podcast_items = document.getElementById("podcast_items");

function fetch_podcast() {
    podcast_title.textContent = "";

    while (podcast_items.firstChild) {
        podcast_items.removeChild(podcast_items.lastChild);
    }

    const selected_podcast = podcasts_select.selectedOptions[0];

    if (selected_podcast === undefined) {
        return;
    }

    fetch(selected_podcast.value).then(response => {
        if (!response.ok) {
            throw response.statusText;
        }

        return response.text();
    }).then(rss_src => {
        const parser = new DOMParser();
        const rss = parser.parseFromString(rss_src, "text/xml");

        const document_element = rss.documentElement;

        const channel = document_element.firstElementChild;

        for (let channel_item of channel.children) {
            switch (channel_item.tagName) {
                case "title":
                    podcast_title.textContent = channel_item.textContent;
                    break;
                case "item":
                    const title_text = getChildText(channel_item, "title");
                    const subtitle_text = getChildText(channel_item, "itunes:subtitle");
                    const description_text = getChildText(channel_item, "description");

                    const title_element = document.createElement("h2");
                    title_element.appendChild(document.createTextNode(title_text));
                    podcast_items.appendChild(title_element);

                    if (subtitle_text && subtitle_text != title_text) {
                        const subtitle_element = document.createElement("h3");
                        subtitle_element.appendChild(document.createTextNode(subtitle_text));
                        podcast_items.appendChild(subtitle_element);
                    }

                    if (description_text) {
                        const description_element = document.createElement("p");
                        description_element.appendChild(document.createTextNode(description_text));
                        podcast_items.appendChild(description_element)
                    }

                    const play_button = document.createElement("button");
                    play_button.appendChild(document.createTextNode("Play"));
                    const enclosure = channel_item.getElementsByTagName("enclosure")[0];
                    const link_href = enclosure ? enclosure.getAttribute("url") : channel_item.getElementsByTagName("link")[0].textContent;
                    play_button.addEventListener("click", () => { post("play_url", link_href); });
                    podcast_items.appendChild(play_button);

                    podcast_items.appendChild(document.createElement("hr"));

                    break;
            }
        }
    });
}

podcasts_select.addEventListener("input", fetch_podcast);

function load_podcasts() {
    const current_podcasts_src = window.localStorage.getItem(LOCAL_STORAGE_PODCASTS_KEY);
    return (current_podcasts_src === null) ? [] : JSON.parse(current_podcasts_src);
}

function add_podcast() {
    const podcast_url = new_podcast.value;

    fetch(podcast_url).then(response => {
        if (!response.ok) {
            throw response.statusText;
        }

        return response.text();
    }).then(rss_src => {
        const parser = new DOMParser();
        const rss = parser.parseFromString(rss_src, "text/xml");

        const document_element = rss.documentElement;

        const channel = document_element.firstElementChild;

        const channel_title = getChildText(channel, "title");

        const current_podcasts = load_podcasts();

        current_podcasts.push({
            channel_title: channel_title,
            channel_url: podcast_url,
        });

        window.localStorage.setItem(LOCAL_STORAGE_PODCASTS_KEY, JSON.stringify(current_podcasts));
        reload_podcasts(true);
    });

    new_podcast.value = "";
}

function reload_podcasts(select_last) {
    while (podcasts_select.firstChild) {
        podcasts_select.removeChild(podcasts_select.lastChild);
    }

    const podcasts = load_podcasts();

    for (let index = 0; index < podcasts.length; index++) {
        const podcast = podcasts[index];

        const podcast_element = document.createElement("option");
        podcast_element.appendChild(document.createTextNode(podcast.channel_title));
        podcast_element.setAttribute("value", podcast.channel_url);

        podcasts_select.appendChild(podcast_element);
    }

    if (select_last) {
        podcasts_select.selectedIndex = podcasts_select.options.length - 1;
    }

    fetch_podcast();
}

reload_podcasts();
