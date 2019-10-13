library(tercen)
library(dplyr)
library(uwot)

# http://127.0.0.1:5402/#ds/2ecef2b0b686d7fde25f34eeb8005605/3-1
# options("tercen.workflowId"= "2ecef2b0b686d7fde25f34eeb8005605")
# options("tercen.stepId"= "3-1")
ctx = tercenCtx()

t(ctx$as.matrix())  %>% 
  uwot::umap() %>%
  (function(umap) {
    d = as_tibble(umap)
    names(d)=paste('umap', seq_along(d), sep = '.')
    return(d)
  }) %>% 
  mutate(.ci = seq_len(nrow(.))-1) %>%
  ctx$addNamespace() %>%
  ctx$save()