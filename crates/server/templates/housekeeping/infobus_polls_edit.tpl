{% set title = locale.hk_editar_encuesta_del_infobus %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="page-head"><div></div><div class="actions"><a class="btn" href="{{ hk.path }}/infobus_polls">{{ locale.hk_volver_al_infobus|raw }}</a></div></div>
<div class="card" style="max-width:720px">
  <div class="card-body">
    <form class="form" method="post" action="{{ hk.path }}/infobus_polls/edit?id={{ poll.id }}">
      <input type="hidden" name="hk_token" value="{{ hk.token }}">
      <input type="hidden" name="creator" value="{{ hk.staff.getId() }}">
      <div class="field"><label>{{ locale.hk_pregunta|raw }}</label><input type="text" name="question" required value="{{ poll.getPollData().getQuestion() }}"></div>
      <div class="field">
        <label>{{ locale.hk_respuestas|raw }}</label>
        <div id="answers" class="form">
          {% for answer in poll.getPollData().getAnswers() %}<input name="answers[]" type="text" value="{{ answer }}">{% endfor %}
        </div>
        <div class="form-actions" style="margin-top:6px">
          <button class="btn sm" type="button" onclick="hkAddAnswer()">{{ locale.hk_anadir_respuesta|raw }}</button>
          <button class="btn sm" type="button" onclick="hkRemoveAnswer()">{{ locale.hk_quitar_la_ultima|raw }}</button>
        </div>
      </div>
      <p class="help" style="margin:0">{{ locale.hk_solo_se_puede_editar_una_encuesta_que_todavia_no|raw }}</p>
      <div class="form-actions"><button class="btn primary" type="submit">{{ locale.hk_guardar_encuesta|raw }}</button></div>
    </form>
  </div>
</div>
{% endautoescape %}
<script>
function hkAddAnswer() {
  var box = document.getElementById('answers');
  var input = document.createElement('input');
  input.type = 'text'; input.name = 'answers[]';
  input.placeholder = '{{ locale.hk_js_answer|raw }}' + (box.children.length + 1);
  box.appendChild(input); input.focus();
}
function hkRemoveAnswer() {
  var box = document.getElementById('answers');
  if (box.children.length > 1) box.removeChild(box.lastElementChild);
}
</script>
{% include "housekeeping/base/footer.tpl" %}
