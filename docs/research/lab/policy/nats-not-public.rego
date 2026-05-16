package main

# NATS must not be publicly reachable. The lab's NATS lives on the
# cluster network; ingress is gated by a NetworkPolicy. We reject any
# Service for NATS that uses NodePort or LoadBalancer.

deny contains msg if {
	input.kind == "Service"
	is_nats(input)
	input.spec.type == "NodePort"
	msg := sprintf("Service %q exposes NATS via NodePort; only ClusterIP allowed", [input.metadata.name])
}

deny contains msg if {
	input.kind == "Service"
	is_nats(input)
	input.spec.type == "LoadBalancer"
	msg := sprintf("Service %q exposes NATS via LoadBalancer; only ClusterIP allowed", [input.metadata.name])
}

is_nats(svc) if {
	svc.metadata.name == "nats"
}

is_nats(svc) if {
	svc.metadata.labels["app.kubernetes.io/name"] == "nats"
}
