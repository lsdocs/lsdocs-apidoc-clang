/**
 * @file
 * Vectors, in C, for the documentation site and apidoc-clang's tests.
 */

#ifndef GEO_VEC_H
#define GEO_VEC_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/** The largest length. */
#define VEC_MAX 1024

/** A vector of numbers. */
typedef struct {
    double *data; /**< The numbers. */
    size_t len;   /**< How many. */
} vec_t;

/** An opaque handle to a vector pool. */
typedef struct vec_pool vec_pool;

/** How a vector is ordered. */
enum vec_order {
    VEC_ASC,  /**< Smallest first. */
    VEC_DESC, /**< Largest first. */
};

/** Flags, as bits. */
enum { VEC_SORTED = 1, VEC_UNIQUE = 2 };

/**
 * Makes a vector.
 *
 * \param len How many numbers; at most #VEC_MAX.
 * \return A vector of \p len zeros, or `NULL` when out of memory.
 * \sa vec_free
 */
vec_t *vec_new(size_t len);

/** Frees a vector made by vec_new(). */
void vec_free(vec_t *v);

/** Sorts it, see \ref vec_order. */
void vec_sort(vec_t *v, enum vec_order order);

/** A callback over numbers. */
typedef int (*vec_visit)(double value, void *data);

/** How many vectors there are. */
extern size_t vec_count;

#ifdef __cplusplus
}
#endif

#endif /* GEO_VEC_H */
