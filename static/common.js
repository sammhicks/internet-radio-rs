function post(url, body) {
    let params = {
        method: "POST",
    };

    if (body !== undefined) {
        params.headers = { "Content-Type": "application/json" };
        params.body = body;
    }

    fetch(url, params).catch(console.error);
}
