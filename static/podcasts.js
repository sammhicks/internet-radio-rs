const LOCAL_STORAGE_PODCASTS_KEY = "podcasts";

function getChildText(element, child_name) {
    const child = element.getElementsByTagName(child_name)[0];
    return child ? child.textContent : undefined;
}

const podcasts_select = document.getElementById("podcasts_select");
const new_podcast = document.getElementById("new_podcast");

const podcast_title = document.getElementById("podcast_title");
const podcast_items = document.getElementById("podcast_items");

podcasts_select.addEventListener("input", ev => {
    podcast_title.textContent = "";

    while (podcast_items.firstChild) {
        podcast_items.removeChild(podcast_items.lastChild);
    }

    fetch(ev.target.selectedOptions[0].value).then(response => {
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

                    const title_element = document.createElement("dt");
                    title_element.appendChild(document.createTextNode(title_text));
                    podcast_items.appendChild(title_element);

                    if (subtitle_text && subtitle_text != title_text) {
                        const subtitle_element = document.createElement("dd");
                        subtitle_element.appendChild(document.createTextNode(subtitle_text));
                        podcast_items.appendChild(subtitle_element);
                    }

                    if (description_text) {
                        const description_element = document.createElement("dd");
                        description_element.appendChild(document.createTextNode(description_text));
                        podcast_items.appendChild(description_element)
                    }

                    const link_dd = document.createElement("dd");
                    const play_button = document.createElement("button");
                    play_button.appendChild(document.createTextNode("Play"));
                    const enclosure = channel_item.getElementsByTagName("enclosure")[0];
                    const link_href = enclosure ? enclosure.getAttribute("url") : channel_item.getElementsByTagName("link")[0].textContent;
                    play_button.addEventListener("click", () => { post("play_url", link_href); });
                    link_dd.appendChild(play_button);
                    podcast_items.appendChild(link_dd);

                    break;
            }
        }
    })
});

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
        reload_podcasts();
    });

    new_podcast.value = "";
}

function reload_podcasts() {
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
}

reload_podcasts();
