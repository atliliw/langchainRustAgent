import json, urllib.request

body = json.dumps({"query": "RAG", "top_k": 5}).encode()
req = urllib.request.Request(
    "http://localhost:8090/api/search/bm25",
    data=body,
    method="POST",
    headers={"Content-Type": "application/json"}
)
resp = urllib.request.urlopen(req, timeout=10)
data = json.loads(resp.read())
print("Total:", data["total_count"])
for r in data["results"][:3]:
    print(f"  Score={r['score']:.4f} | {r['content'][:80]}")
