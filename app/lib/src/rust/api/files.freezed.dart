// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint, type=warning, deprecated_member_use, deprecated_member_use_from_same_package
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'files.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$TransferEvent {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is TransferEvent);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'TransferEvent()';
}


}

/// @nodoc
class $TransferEventCopyWith<$Res>  {
$TransferEventCopyWith(TransferEvent _, $Res Function(TransferEvent) __);
}


/// Adds pattern-matching-related methods to [TransferEvent].
extension TransferEventPatterns on TransferEvent {
/// A variant of `map` that fallback to returning `orElse`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( TransferEvent_Started value)?  started,TResult Function( TransferEvent_Progress value)?  progress,TResult Function( TransferEvent_Finished value)?  finished,TResult Function( TransferEvent_Failed value)?  failed,required TResult orElse(),}){
final _that = this;
switch (_that) {
case TransferEvent_Started() when started != null:
return started(_that);case TransferEvent_Progress() when progress != null:
return progress(_that);case TransferEvent_Finished() when finished != null:
return finished(_that);case TransferEvent_Failed() when failed != null:
return failed(_that);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// Callbacks receives the raw object, upcasted.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case final Subclass2 value:
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( TransferEvent_Started value)  started,required TResult Function( TransferEvent_Progress value)  progress,required TResult Function( TransferEvent_Finished value)  finished,required TResult Function( TransferEvent_Failed value)  failed,}){
final _that = this;
switch (_that) {
case TransferEvent_Started():
return started(_that);case TransferEvent_Progress():
return progress(_that);case TransferEvent_Finished():
return finished(_that);case TransferEvent_Failed():
return failed(_that);}
}
/// A variant of `map` that fallback to returning `null`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( TransferEvent_Started value)?  started,TResult? Function( TransferEvent_Progress value)?  progress,TResult? Function( TransferEvent_Finished value)?  finished,TResult? Function( TransferEvent_Failed value)?  failed,}){
final _that = this;
switch (_that) {
case TransferEvent_Started() when started != null:
return started(_that);case TransferEvent_Progress() when progress != null:
return progress(_that);case TransferEvent_Finished() when finished != null:
return finished(_that);case TransferEvent_Failed() when failed != null:
return failed(_that);case _:
  return null;

}
}
/// A variant of `when` that fallback to an `orElse` callback.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( int transferId)?  started,TResult Function( BigInt done,  BigInt? total)?  progress,TResult Function( BigInt bytes)?  finished,TResult Function( String message)?  failed,required TResult orElse(),}) {final _that = this;
switch (_that) {
case TransferEvent_Started() when started != null:
return started(_that.transferId);case TransferEvent_Progress() when progress != null:
return progress(_that.done,_that.total);case TransferEvent_Finished() when finished != null:
return finished(_that.bytes);case TransferEvent_Failed() when failed != null:
return failed(_that.message);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// As opposed to `map`, this offers destructuring.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case Subclass2(:final field2):
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( int transferId)  started,required TResult Function( BigInt done,  BigInt? total)  progress,required TResult Function( BigInt bytes)  finished,required TResult Function( String message)  failed,}) {final _that = this;
switch (_that) {
case TransferEvent_Started():
return started(_that.transferId);case TransferEvent_Progress():
return progress(_that.done,_that.total);case TransferEvent_Finished():
return finished(_that.bytes);case TransferEvent_Failed():
return failed(_that.message);}
}
/// A variant of `when` that fallback to returning `null`
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( int transferId)?  started,TResult? Function( BigInt done,  BigInt? total)?  progress,TResult? Function( BigInt bytes)?  finished,TResult? Function( String message)?  failed,}) {final _that = this;
switch (_that) {
case TransferEvent_Started() when started != null:
return started(_that.transferId);case TransferEvent_Progress() when progress != null:
return progress(_that.done,_that.total);case TransferEvent_Finished() when finished != null:
return finished(_that.bytes);case TransferEvent_Failed() when failed != null:
return failed(_that.message);case _:
  return null;

}
}

}

/// @nodoc


class TransferEvent_Started extends TransferEvent {
  const TransferEvent_Started({required this.transferId}): super._();
  

 final  int transferId;

/// Create a copy of TransferEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$TransferEvent_StartedCopyWith<TransferEvent_Started> get copyWith => _$TransferEvent_StartedCopyWithImpl<TransferEvent_Started>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is TransferEvent_Started&&(identical(other.transferId, transferId) || other.transferId == transferId));
}


@override
int get hashCode {
    return Object.hash(runtimeType,transferId);
}

@override
String toString() {
    return 'TransferEvent.started(transferId: $transferId)';
}


}

/// @nodoc
abstract mixin class $TransferEvent_StartedCopyWith<$Res> implements $TransferEventCopyWith<$Res> {
  factory $TransferEvent_StartedCopyWith(TransferEvent_Started value, $Res Function(TransferEvent_Started) _then) = _$TransferEvent_StartedCopyWithImpl;
@useResult
$Res call({
 int transferId
});




}
/// @nodoc
class _$TransferEvent_StartedCopyWithImpl<$Res>
    implements $TransferEvent_StartedCopyWith<$Res> {
  _$TransferEvent_StartedCopyWithImpl(this._self, this._then);

  final TransferEvent_Started _self;
  final $Res Function(TransferEvent_Started) _then;

/// Create a copy of TransferEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? transferId = null,}) {
  return _then(TransferEvent_Started(
transferId: null == transferId ? _self.transferId : transferId // ignore: cast_nullable_to_non_nullable
as int,
  ));
}


}

/// @nodoc


class TransferEvent_Progress extends TransferEvent {
  const TransferEvent_Progress({required this.done, this.total}): super._();
  

 final  BigInt done;
 final  BigInt? total;

/// Create a copy of TransferEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$TransferEvent_ProgressCopyWith<TransferEvent_Progress> get copyWith => _$TransferEvent_ProgressCopyWithImpl<TransferEvent_Progress>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is TransferEvent_Progress&&(identical(other.done, done) || other.done == done)&&(identical(other.total, total) || other.total == total));
}


@override
int get hashCode {
    return Object.hash(runtimeType,done,total);
}

@override
String toString() {
    return 'TransferEvent.progress(done: $done, total: $total)';
}


}

/// @nodoc
abstract mixin class $TransferEvent_ProgressCopyWith<$Res> implements $TransferEventCopyWith<$Res> {
  factory $TransferEvent_ProgressCopyWith(TransferEvent_Progress value, $Res Function(TransferEvent_Progress) _then) = _$TransferEvent_ProgressCopyWithImpl;
@useResult
$Res call({
 BigInt done, BigInt? total
});




}
/// @nodoc
class _$TransferEvent_ProgressCopyWithImpl<$Res>
    implements $TransferEvent_ProgressCopyWith<$Res> {
  _$TransferEvent_ProgressCopyWithImpl(this._self, this._then);

  final TransferEvent_Progress _self;
  final $Res Function(TransferEvent_Progress) _then;

/// Create a copy of TransferEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? done = null,Object? total = freezed,}) {
  return _then(TransferEvent_Progress(
done: null == done ? _self.done : done // ignore: cast_nullable_to_non_nullable
as BigInt,total: freezed == total ? _self.total : total // ignore: cast_nullable_to_non_nullable
as BigInt?,
  ));
}


}

/// @nodoc


class TransferEvent_Finished extends TransferEvent {
  const TransferEvent_Finished({required this.bytes}): super._();
  

 final  BigInt bytes;

/// Create a copy of TransferEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$TransferEvent_FinishedCopyWith<TransferEvent_Finished> get copyWith => _$TransferEvent_FinishedCopyWithImpl<TransferEvent_Finished>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is TransferEvent_Finished&&(identical(other.bytes, bytes) || other.bytes == bytes));
}


@override
int get hashCode {
    return Object.hash(runtimeType,bytes);
}

@override
String toString() {
    return 'TransferEvent.finished(bytes: $bytes)';
}


}

/// @nodoc
abstract mixin class $TransferEvent_FinishedCopyWith<$Res> implements $TransferEventCopyWith<$Res> {
  factory $TransferEvent_FinishedCopyWith(TransferEvent_Finished value, $Res Function(TransferEvent_Finished) _then) = _$TransferEvent_FinishedCopyWithImpl;
@useResult
$Res call({
 BigInt bytes
});




}
/// @nodoc
class _$TransferEvent_FinishedCopyWithImpl<$Res>
    implements $TransferEvent_FinishedCopyWith<$Res> {
  _$TransferEvent_FinishedCopyWithImpl(this._self, this._then);

  final TransferEvent_Finished _self;
  final $Res Function(TransferEvent_Finished) _then;

/// Create a copy of TransferEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? bytes = null,}) {
  return _then(TransferEvent_Finished(
bytes: null == bytes ? _self.bytes : bytes // ignore: cast_nullable_to_non_nullable
as BigInt,
  ));
}


}

/// @nodoc


class TransferEvent_Failed extends TransferEvent {
  const TransferEvent_Failed({required this.message}): super._();
  

 final  String message;

/// Create a copy of TransferEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$TransferEvent_FailedCopyWith<TransferEvent_Failed> get copyWith => _$TransferEvent_FailedCopyWithImpl<TransferEvent_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is TransferEvent_Failed&&(identical(other.message, message) || other.message == message));
}


@override
int get hashCode {
    return Object.hash(runtimeType,message);
}

@override
String toString() {
    return 'TransferEvent.failed(message: $message)';
}


}

/// @nodoc
abstract mixin class $TransferEvent_FailedCopyWith<$Res> implements $TransferEventCopyWith<$Res> {
  factory $TransferEvent_FailedCopyWith(TransferEvent_Failed value, $Res Function(TransferEvent_Failed) _then) = _$TransferEvent_FailedCopyWithImpl;
@useResult
$Res call({
 String message
});




}
/// @nodoc
class _$TransferEvent_FailedCopyWithImpl<$Res>
    implements $TransferEvent_FailedCopyWith<$Res> {
  _$TransferEvent_FailedCopyWithImpl(this._self, this._then);

  final TransferEvent_Failed _self;
  final $Res Function(TransferEvent_Failed) _then;

/// Create a copy of TransferEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? message = null,}) {
  return _then(TransferEvent_Failed(
message: null == message ? _self.message : message // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

// dart format on
