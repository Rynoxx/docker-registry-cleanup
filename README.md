Simple utility to delete images from a container registry.  
It is still neccessary to run the [garbage collection](https://distribution.github.io/distribution/about/garbage-collection/) separately to remove files on disk.  
Use `-t` and `-i` to filter tags/images with regex.  
Specify the number of tags per matching filter to keep with `-m`.  
This assumes that tags are either lexographically sortable or semver sortable to determine which tag is the latest.   

Runs a dry-run by default, specify `-d`/`--delete` to actually run the deletions.  

If --semver is specified all tags which don't match a valid semver are ignored.  

Sometimes it's neccessary to run the cleanup multiple times with different paramters to match all different criterias.
Depending on how coherent the naming scheme for your tags are.

Implemented the bare minimum of what was actually needed for my use case.

### How to run it:

To run this manually as a docker container
```bash
docker run --rm ghcr.io/rynoxx/docker-registry-cleanup:latest -r "https://docker-registry.example.com/" -t "^dev-.*" -m 5
```

To run as a scheduled cron job in kubernetes:
```yaml
apiVersion: batch/v1
kind: CronJob
metadata:
  name: registry-cleanup
spec:
  schedule: "30 0 * * *"  # every night at 00:30
  jobTemplate:
    spec:
      template:
        spec:
          containers:
          - name: registry-cleanup
            image: ghcr.io/rynoxx/docker-registry-cleanup:latest
            args: # Keep the latest 5 tags each that start with 'dev-' or 'test-'
              - -r
              - "https://docker-registry.example.com/"
              - -t
              - "^dev-.*"
              - -t
              - "^test-.*"
              - -m
              - "5"
              #- -d # Specify -d to actually do deletions.
```

### Command help:
```
# ./docker-registry-cleanup -h
Mark things for deletion, you'll have to run the garbage collection yourself

Usage: docker-registry-cleanup [OPTIONS] --registry-url <REGISTRY_URL> --max-per-tag <MAX_PER_TAG>

Options:
  -r, --registry-url <REGISTRY_URL>
          The base URL of the container registry. e.g. https://docker.io/
      --registry-user <REGISTRY_USER>
          Optional username to use when logging in to the registry
      --registry-password <REGISTRY_PASSWORD>
          Optional password to use when logging in to the registry
  -m, --max-per-tag <MAX_PER_TAG>
          Maximum number of images to keep per tag and regex pattern
  -t, --tags <TAGS>
          Regex for tag whitelist, multiple can be specified if any match then it's in whitelist. If none, no action is taken.
          The max_per_tag is applied per pattern here. Specifying two will result in two separate lists of tags for max_per_tag
  -i, --images <IMAGES>
          Regex for image whitelist, multiple can be specified if any of them match then it's in whitelist. If none all images are whitelisted
  -s, --semver
          Should the tags be sorted by semver? Ignores any non-semver tags
  -d, --delete
          Run actual deletions. Otherwise it's dry-run by default
  -h, --help
          Print help
  -V, --version
          Print version
```

