Simple utility to delete images from a container registry.  
Use `-t` and `-i` to filter tags/images with regex.  
Specify the number of tags per matching filter to keep with `-m`.  

If --semver is specified it will ignore all tags which don't match a valid semver string.  

Sometimes it'd be neccessary to run it multiple times with different paramters to clean all neccessary things.

TODO: Actually fill this out with proper instructions.

Implemented the bare minimum of what was actually needed for my use case.

To run this manually in a docker container
```bash
docker run --rm <docker-image-path:TBA> ./docker-registry-cleanup -r "https://docker-registry.example.com/" -t "^dev-.*" -m 5
```

To run as a scheduled cron job:
```yaml
<TODO>
```

