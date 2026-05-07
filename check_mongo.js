var dbs = db.adminCommand("listDatabases");
dbs.databases.forEach(function(d) {
  print("DB: " + d.name + " (" + d.sizeOnDisk + " bytes)");
});

var dbNames = ["langchainrust", "langchainrust_demo"];
dbNames.forEach(function(name) {
  var conn = db.getSiblingDB(name);
  var cols = conn.getCollectionNames();
  if (cols.length > 0) {
    print("\n=== " + name + " ===");
    cols.forEach(function(col) {
      print("  " + col + ": " + conn.getCollection(col).countDocuments({}) + " docs");
    });
  }
});
