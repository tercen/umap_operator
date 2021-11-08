library(tercen)
library(dplyr)
library(uwot)

#workspace folder
######
options("tercen.workflowId"= "09d4e7ebbf6268821d73acf90c0006b5")
options("tercen.stepId"= "ea0882b5-785f-4254-a715-8c25ce7e4647")
getOption("tercen.workflowId")
getOption("tercen.stepId")
######

ctx <- tercenCtx()

pca <- NULL
if (as.character(ctx$op.value('pca')) != "NULL")  pca  = as.integer(ctx$op.value('pca'))

seed <- NULL
if(!ctx$op.value('seed') < 0) seed <- as.integer(ctx$op.value('seed'))

set.seed(seed)

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
