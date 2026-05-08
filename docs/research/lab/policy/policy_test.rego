package main

# Unit tests for the lab's OPA policies.
# Run with: opa test policy/

# ── encrypted-storage-required.rego ────────────────────────────────────
test_db_key_from_secret_passes if {
	count(deny) == 0 with input as {
		"kind": "Deployment",
		"metadata": {"name": "openstory"},
		"spec": {"template": {"spec": {"containers": [{
			"name": "openstory",
			"env": [{
				"name": "OPEN_STORY_DB_KEY",
				"valueFrom": {"secretKeyRef": {"name": "openstory-secrets", "key": "db_key"}},
			}],
		}]}}},
	}
}

test_db_key_inline_fails if {
	count(deny) > 0 with input as {
		"kind": "Deployment",
		"metadata": {"name": "openstory"},
		"spec": {"template": {"spec": {"containers": [{
			"name": "openstory",
			"env": [{"name": "OPEN_STORY_DB_KEY", "value": "literally-here"}],
		}]}}},
	}
}

test_db_key_missing_fails if {
	count(deny) > 0 with input as {
		"kind": "Deployment",
		"metadata": {"name": "openstory"},
		"spec": {"template": {"spec": {"containers": [{"name": "openstory", "env": []}]}}},
	}
}

# ── api-token-required.rego ────────────────────────────────────────────
test_api_token_from_secret_passes if {
	count(deny) == 0 with input as {
		"kind": "Deployment",
		"metadata": {"name": "openstory"},
		"spec": {"template": {"spec": {"containers": [{
			"name": "openstory",
			"env": [
				{
					"name": "OPEN_STORY_API_TOKEN",
					"valueFrom": {"secretKeyRef": {"name": "openstory-secrets", "key": "api_token"}},
				},
				{
					"name": "OPEN_STORY_DB_KEY",
					"valueFrom": {"secretKeyRef": {"name": "openstory-secrets", "key": "db_key"}},
				},
			],
		}]}}},
	}
}

# ── no-token-in-url.rego ───────────────────────────────────────────────
test_nats_url_clean_passes if {
	count(deny) == 0 with input as {
		"kind": "ConfigMap",
		"metadata": {"name": "openstory-config"},
		"data": {"NATS_URL": "nats://nats.openstory.svc.cluster.local:4222"},
	}
}

test_nats_url_with_userinfo_fails if {
	count(deny) > 0 with input as {
		"kind": "ConfigMap",
		"metadata": {"name": "openstory-config"},
		"data": {"NATS_URL": "nats://token@nats.openstory.svc.cluster.local:4222"},
	}
}

# ── nats-not-public.rego ───────────────────────────────────────────────
test_nats_clusterip_passes if {
	count(deny) == 0 with input as {
		"kind": "Service",
		"metadata": {"name": "nats", "labels": {"app.kubernetes.io/name": "nats"}},
		"spec": {"type": "ClusterIP", "ports": [{"port": 4222}]},
	}
}

test_nats_nodeport_fails if {
	count(deny) > 0 with input as {
		"kind": "Service",
		"metadata": {"name": "nats", "labels": {"app.kubernetes.io/name": "nats"}},
		"spec": {"type": "NodePort", "ports": [{"port": 4222}]},
	}
}

test_nats_loadbalancer_fails if {
	count(deny) > 0 with input as {
		"kind": "Service",
		"metadata": {"name": "nats", "labels": {"app.kubernetes.io/name": "nats"}},
		"spec": {"type": "LoadBalancer", "ports": [{"port": 4222}]},
	}
}
