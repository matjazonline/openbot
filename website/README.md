# BusyBots website

Static website served by non-root Nginx on port 8080. Fly terminates HTTPS.
The Fly configuration uses a shared CPU with 256 MB RAM in Frankfurt and
automatically stops the machine when idle, restarting it on incoming requests.

First deployment (from this directory):

```sh
fly auth login
fly apps create busybots-website
fly deploy --remote-only --ha=false
```

The app name must be available in your account; if it is taken, choose another
name and update `app` in `fly.toml` before deploying. `--ha=false` prevents Fly
from provisioning a second machine on the initial deployment.

Subsequent deployments:

```sh
fly deploy --remote-only --ha=false
```

Verify with `fly status` and `fly checks list`, then open the app's HTTPS URL
and check the page and images. This deployment needs no volumes or dedicated IPv4.
