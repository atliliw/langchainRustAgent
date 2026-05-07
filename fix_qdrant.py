import json, urllib.request
data = json.dumps({"optimizers_config": {"indexing_threshold": 10}}).encode()
req = urllib.request.Request(
    "http://localhost:6333/collections/demo_documents",
    data=data,
    method="PATCH",
    headers={"Content-Type": "application/json"}
)
resp = urllib.request.urlopen(req)
print(resp.read().decode())
