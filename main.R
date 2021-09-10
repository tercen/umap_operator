library(tercen)
library(dplyr)
library(uwot)


ctx <- tercenCtx()

pca <- NULL
if (as.character(ctx$op.value('pca')) != "NULL")  pca  = as.integer(ctx$op.value('pca'))

t(ctx$as.matrix())  %>% 
  uwot::umap(init     = as.character(ctx$op.value('init')),
             scale    = as.character(ctx$op.value('scale')),
             spread   = as.double(ctx$op.value('spread')),
             min_dist = as.double(ctx$op.value('min_dist')),
             pca      = pca
            ) %>%
  (function(umap) {
    d = as_tibble(umap,.name_repair = NULL)
    names(d)=paste('umap', seq_along(d), sep = '.')
    return(d)
  }) %>% 
  mutate(.ci = seq_len(nrow(.))-1) %>%
  ctx$addNamespace() %>%
  ctx$save()
