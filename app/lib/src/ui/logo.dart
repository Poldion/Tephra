import 'package:material_ui/material_ui.dart';

/// Das Zeichen von Tephra: Eingabezeichen und Schreibmarke, darüber
/// aufsteigende Bruchstücke. Die Geometrie entspricht dem App-Symbol
/// (`tool/make_icons.py`), hier ohne Kachel.
class TephraLogo extends StatelessWidget {
  const TephraLogo({super.key, required this.size});

  final double size;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final dark = scheme.brightness == Brightness.dark;
    return CustomPaint(
      size: Size.square(size),
      painter: _LogoPainter(
        prompt: scheme.primary,
        // Bernstein wie im App-Symbol; auf hellem Grund etwas dunkler, damit es lesbar bleibt.
        shard: dark ? const Color(0xFFFFB454) : const Color(0xFFD9861A),
      ),
    );
  }
}

class _LogoPainter extends CustomPainter {
  const _LogoPainter({required this.prompt, required this.shard});

  final Color prompt;
  final Color shard;

  // Ausschnitt des Rasters von 108 Einheiten, der das Zeichen knapp umschließt.
  static const _from = Offset(26, 16.5);
  static const _extent = 62.0;

  static const _shards = [
    [Offset(61, 40), Offset(67, 46), Offset(61, 52), Offset(55, 46)],
    [Offset(69, 30), Offset(79, 34), Offset(72, 41)],
    [Offset(63, 23), Offset(66.5, 26.5), Offset(63, 30), Offset(59.5, 26.5)],
    [Offset(81, 22), Offset(84, 25), Offset(81, 28), Offset(78, 25)],
  ];

  @override
  void paint(Canvas canvas, Size size) {
    final scale = size.shortestSide / _extent;
    canvas
      ..save()
      ..scale(scale)
      ..translate(-_from.dx, -_from.dy);
    final stroke = Paint()
      ..color = prompt
      ..style = PaintingStyle.stroke
      ..strokeWidth = 7.5
      ..strokeCap = StrokeCap.round
      ..strokeJoin = StrokeJoin.round;
    canvas
      ..drawPath(
        Path()
          ..moveTo(33, 41)
          ..lineTo(50, 55)
          ..lineTo(33, 69),
        stroke,
      )
      ..drawLine(const Offset(57, 69), const Offset(77, 69), stroke);
    final fill = Paint()..color = shard;
    for (final points in _shards) {
      canvas.drawPath(Path()..addPolygon(points, true), fill);
    }
    canvas.restore();
  }

  @override
  bool shouldRepaint(_LogoPainter old) => old.prompt != prompt || old.shard != shard;
}
