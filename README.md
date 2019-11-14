# umap operator

#### Description
`umap` operator performs umap analysis.

##### Usage
Input projection|.
---|---
`row`   | represents the variables (e.g. genes, channels, markers)
`col`   | represents the observations (e.g. cells, samples, individuals) 
`y-axis`| measurement value


Input parameters|.
---|---
`init`           | character, type of initialization for the coordinates, see details
`scale`          | numeric, type of scaling to apply to data
`spread`         | numeric, the effective scale of embedded points. In combination with `min_dist`, this determines how clustered/clumped the embedded points are
`min_dist`       | numeric, the effective minimum distance between embedded point 
`pca`            | numeric, If set to a positive integer value, reduce data to this number of columns using PCA



Output relations|.
---|---
`umap01, umap02`| first two components containing the new projected values


##### Details

The operator performs umap analysis. It reduces the amount of variables (i.e. indicated by rows) to a lower number (default 2). This operators wraps the `uwot::umap()`. See (https://github.com/jlmelville/uwot) for more details, especially settings and examples.


#### Reference

##### See Also

[pca](https://github.com/tercen/pca_operator), [tsne](https://github.com/tercen/tsne_operator)

#### Examples

