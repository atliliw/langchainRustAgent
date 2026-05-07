#!/bin/bash
cat > /tmp/update_qdrant.json << 'JSONEOF'
{"optimizers_config": {"indexing_threshold": 10}}
JSONEOF
curl -s -X PATCH "http://localhost:6333/collections/demo_documents" \
  -H "Content-Type: application/json" \
  -d @/tmp/update_qdrant.json
