package main

# Conftest policy: every Deployment named "openstory" must source
# OPEN_STORY_DB_KEY from a Kubernetes Secret via secretKeyRef.
# This guarantees SQLCipher is enabled with a non-empty key.
#
# Run against rendered Helm output:
#   helm template charts/openstory > rendered.yaml
#   conftest test rendered.yaml --policy policy/

deny contains msg if {
	input.kind == "Deployment"
	contains_lower(input.metadata.name, "openstory")
	container := input.spec.template.spec.containers[_]
	not has_secret_env(container, "OPEN_STORY_DB_KEY")
	msg := sprintf(
		"Deployment %q container %q does not set OPEN_STORY_DB_KEY from a secretKeyRef — encryption at rest required",
		[input.metadata.name, container.name],
	)
}

deny contains msg if {
	input.kind == "Deployment"
	contains_lower(input.metadata.name, "openstory")
	container := input.spec.template.spec.containers[_]
	env := container.env[_]
	env.name == "OPEN_STORY_DB_KEY"
	env.value != ""
	# inline value, not secretKeyRef → forbidden
	not env.valueFrom.secretKeyRef.name
	msg := sprintf(
		"OPEN_STORY_DB_KEY in Deployment %q is set inline; must come from a Secret",
		[input.metadata.name],
	)
}

# helpers
has_secret_env(container, name) if {
	env := container.env[_]
	env.name == name
	env.valueFrom.secretKeyRef.name != ""
}

contains_lower(s, sub) if {
	contains(lower(s), lower(sub))
}
