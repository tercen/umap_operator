## Deploy

```R
renv::init()
```

#!/usr/bin/env bash

```
git add -A && git commit -m "touched up docs" && git push
git tag -a 0.0.2 -m "++" && git push && git push --tags
```

