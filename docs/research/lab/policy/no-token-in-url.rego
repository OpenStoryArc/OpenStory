package main

# The NATS URL must never contain inline credentials (no `user:pass@`
# or `token@` userinfo). Auth happens via mTLS / nkey / user-password
# at the NATS layer, not via the URL.

deny contains msg if {
	input.kind == "ConfigMap"
	url := input.data.NATS_URL
	contains(url, "@")
	msg := sprintf(
		"NATS_URL in ConfigMap %q contains inline credentials (`@` in URL); use NATS auth not URL auth",
		[input.metadata.name],
	)
}

deny contains msg if {
	input.kind == "Deployment"
	container := input.spec.template.spec.containers[_]
	env := container.env[_]
	env.name == "NATS_URL"
	contains(env.value, "@")
	msg := sprintf(
		"NATS_URL env in Deployment %q container %q contains inline credentials",
		[input.metadata.name, container.name],
	)
}
