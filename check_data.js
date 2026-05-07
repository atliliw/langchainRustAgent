var dbNames = ["langchainrust", "langchainrust_demo"];
dbNames.forEach(function(name) {
  var conn = db.getSiblingDB(name);
  var cols = conn.getCollectionNames();
  if (cols.length > 0) {
    print("=== " + name + " ===");
    cols.forEach(function(col) {
      print("  " + col + ": " + conn.getCollection(col).countDocuments({}) + " docs");
    });
  }
});
