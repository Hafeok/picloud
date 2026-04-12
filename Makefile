.PHONY: cluster-build cluster-up cluster-down cluster-clean cluster-logs cluster-status cluster-restart

cluster-build:
	docker compose build

cluster-up:
	docker compose up -d

cluster-down:
	docker compose down

cluster-clean:
	docker compose down -v

cluster-logs:
	docker compose logs -f

cluster-status:
	@echo "=== node1 (localhost:7443) ==="
	@curl -sf http://localhost:7443/health 2>/dev/null || echo "unreachable"
	@echo ""
	@echo "=== node2 (localhost:7444) ==="
	@curl -sf http://localhost:7444/health 2>/dev/null || echo "unreachable"
	@echo ""
	@echo "=== node3 (localhost:7445) ==="
	@curl -sf http://localhost:7445/health 2>/dev/null || echo "unreachable"

cluster-restart: cluster-down cluster-up
