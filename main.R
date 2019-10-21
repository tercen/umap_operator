library(tercen)
library(dplyr)
library(uwot)

# http://127.0.0.1:5402/#ds/2ecef2b0b686d7fde25f34eeb8005605/3-1
# options("tercen.workflowId"= "2ecef2b0b686d7fde25f34eeb8005605")
# options("tercen.stepId"= "3-1")
ctx = tercenCtx()

pca = NULL
# if (as.character(ctx$op.value('pca')) != "NULL")  pca  = as.integer(ctx$op.value('pca'))

t(ctx$as.matrix())  %>% 
  uwot::umap(init     = as.character(ctx$op.value('init')),
             scale    = as.character(ctx$op.value('scale')),
             spread   = as.double(ctx$op.value('spread')),
             min_dist = as.double(ctx$op.value('min_dist')),
             pca      = pca
            ) %>%
  (function(umap) {
    d = as_tibble(umap)
    names(d)=paste('umap', seq_along(d), sep = '.')
    return(d)
  }) %>% 
  mutate(.ci = seq_len(nrow(.))-1) %>%
  ctx$addNamespace() %>%
  ctx$save()