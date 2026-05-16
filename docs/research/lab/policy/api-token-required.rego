package main

# Every OpenStory Deployment must source OPEN_STORY_API_TOKEN from a
# Secret via secretKeyRef. An empty bearer token = no auth = the lab
# is wide open. We refuse to render that configuration.

deny contains msg if {
	input.kind == "Deployment"
	contains_lower(input.metadata.name, "openstory")
	container := input.spec.template.spec.containers[_]
	not has_secret_env_local(container, "OPEN_STORY_API_TOKEN")
	msg := sprintf(
		"Deployment %q container %q does not set OPEN_STORY_API_TOKEN from a secretKeyRef — bearer auth required",
		[input.metadata.name, container.name],
	)
}

has_secret_env_local(container, name) if {
	env := container.env[_]
	env.name == name
	env.valueFrom.secretKeyRef.name != ""
}

contains_lower(s, sub) if {
	contains(lower(s), lower(sub))
}
