use("langchainrust");
var docs = db.bm25_chunks.find({"metadata.original_filename": /02-RAG/}).limit(3).toArray();
print("Found BM25 chunks: " + docs.length);
docs.forEach(function(d) {
  print("filename: " + (d.metadata ? d.metadata.original_filename : "N/A"));
  print("preview: " + (d.content ? d.content.substring(0,80) : "N/A"));
  print("parent_id: " + (d.parent_id || "N/A"));
  print("---");
});
print("Total documents (by filename):");
db.bm25_chunks.aggregate([
  {$group: {_id: "$metadata.original_filename", count: {$sum: 1}}},
  {$sort: {count: -1}},
  {$limit: 20}
]).forEach(function(g) {
  print("  " + (g._id || "null") + ": " + g.count + " chunks");
});
