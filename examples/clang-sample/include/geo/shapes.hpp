/**
 * @file
 * Shapes, for the documentation site and apidoc-clang's tests.
 */

#pragma once

/// Paints with a color.
#define GEO_PAINT(shape, color) ((shape).paint(color))

/// The library's version.
#define GEO_VERSION 3

namespace geo {

/// Ways to paint.
enum class Color : unsigned char {
    Red = 1,   ///< Warm.
    Green,     ///< Calm.
    Blue = 7,  ///< Cool.
};

/// A point in the plane.
///
/// Points are values: copy them freely.
struct Point {
    double x = 0;  ///< Across.
    double y = 0;  ///< Up.

    /// How far from the origin.
    double norm() const noexcept;
};

/**
 * Anything with an area, see @ref Circle.
 *
 * Derive from it and implement @c area().
 */
class Shape {
public:
    /// Makes one at `center`.
    explicit Shape(Point center);

    /// Copies one.
    Shape(const Shape &other) = default;

    virtual ~Shape();

    /**
     * The area.
     * @return The area, in square units.
     */
    virtual double area() const = 0;

    /// Paints it.
    /// @param color The color.
    /// @param times How many coats.
    void paint(Color color, int times = 1);

    /// Paints it by name.
    void paint(const char *name);

    /// How many there are.
    static unsigned long count;

protected:
    /// Where it is.
    Point center_;

private:
    int coats_ = 0;
};

/// A round shape.
class Circle final : public Shape {
public:
    /**
     * Makes one.
     * @param center Where.
     * @param radius How big.
     * @throws invalid_argument If `radius` is negative.
     */
    Circle(Point center, double radius);

    double area() const override;

    /// The radius.
    double radius() const { return radius_; }

private:
    double radius_;
};

/**
 * The largest of some shapes.
 *
 * @tparam It An iterator over shape pointers.
 * @param first The first.
 * @param last Past the last.
 * @return The largest, or `last` if there are none.
 * @deprecated Use @ref largest_of instead.
 */
template <typename It>
It largest(It first, It last);

/// A list of things.
template <typename T>
struct List {
    T *items;            ///< The things.
    unsigned long size;  ///< How many.
};

/// Shapes, by pointer.
using Registry = List<Shape *>;

/// How many sides a regular shape has.
template <int N>
struct Sides {
    /// The number.
    static constexpr int value = N;
};

/// The circle's constant.
constexpr double pi = 3.14159265358979;

namespace detail {
/// Not for users.
void helper();
}  // namespace detail

}  // namespace geo
