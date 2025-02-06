library(tercen)
library(dplyr)
library(uwot)

ctx <- tercenCtx()

pca <- ctx$op.value('pca', as.integer, -1)
if(pca <= 0) pca <- NULL

seed <- ctx$op.value('seed', as.integer, -1)
if(seed > 0) set.seed(seed)

mat <- t(ctx$as.matrix()) 

prop.train <- ctx$op.value('prop.train', as.double, 1)
if(prop.train > 1 | prop.train <= 0) stop("prop.train must be between 0 and 1")

if(prop.train == 1) {
  mat.train <- mat
} else {
  size <- nrow(mat)
  idx <- sample(x = seq_len(size), size = ceiling(size * prop.train))
  mat.train <- mat[idx, ]
}

umap_train <- mat.train %>% 
  uwot::umap(
    init     = ctx$op.value('init', as.character, "spectral"),
    scale    = ctx$op.value('scale', as.character, "none"),
    spread   = ctx$op.value('spread', as.double, 1),
    min_dist = ctx$op.value('min_dist', as.double, 0.01),
    n_neighbors = ctx$op.value('n_neighbors', as.double, 15),
    pca      = pca,
    ret_model = TRUE
  )

if(prop.train == 1) {
  embeddings_full <- umap_train$embedding
} else {
  mat.test <-  mat[-idx, ]
  umap_test <- umap_transform(mat.test, umap_train)
  ord <- order(c(idx, seq_len(size)[-idx]))
  embeddings_full <- rbind(umap_train$embedding, umap_test)[ord, ]
}

embeddings_full %>%
  (function(umap) {
    d = as_tibble(umap)
    names(d) = paste('umap', seq_along(d), sep = '.')
    return(d)
  }) %>% 
  mutate(.ci = seq_len(nrow(.)) - 1L) %>%
  ctx$addNamespace() %>%
  as_relation() %>%
  as_join_operator(ctx$rnames, ctx$rnames) %>%
  save_relation(ctx)
