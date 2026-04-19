import modal

app = modal.App("converge-llm-worker")

image = (
    modal.Image.debian_slim()
    .apt_install("curl", "ca-certificates")
)


@app.function(
    image=image,
    gpu="L4",
    timeout=60 * 60,
    secrets=[],
)
def healthcheck():
    return {
        "service": "converge-llm-worker",
        "status": "prepared",
        "note": "Replace this function with a real converge-llm-server launch flow.",
    }
